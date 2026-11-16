//! Checking a connection step by step (adapter, runtime, credentials, a session and a trial prompt)
//! and signing in to an agent account.

use super::*;

impl App {
    /// Start a configuration check in the background, so it finishes even if the app that asked goes away.
    pub fn start_check(self: &Arc<Self>, id: &str) -> Result<Value> {
        let mut c = self.connection(id)?;
        if c["status"] == "testing" && c["testStartedAt"].as_i64().is_some_and(|t| now_ms() - t < 200_000) {
            return Ok(c);
        }
        c["status"] = json!("testing");
        c["testStartedAt"] = json!(now_ms());
        c["check"] = json!([]);
        self.db.put_config(CONNECTION, id, &c, now_ms())?;
        let app = self.clone();
        let id = id.to_string();
        tokio::spawn(async move {
            if let Err(err) = app.test_connection(&id).await {
                crate::config::log(format!("janus: check {id} failed to record: {err:#}"));
            }
        });
        Ok(c)
    }

    /// A check left half-done by a restart reads as "not tested" again.
    pub fn reset_stale_checks(&self) -> Result<()> {
        for mut c in self.db.list_config(CONNECTION)?.into_iter().filter(|c| c["status"] == "testing") {
            c["status"] = json!("untested");
            self.db.put_config(CONNECTION, str_of(&c, "id"), &c, now_ms())?;
        }
        Ok(())
    }

    /// Record one step of the check so the app can show progress while it runs.
    pub(super) fn check_step(&self, id: &str, steps: &mut Vec<Value>, name: &str, ok: Option<bool>, detail: String) {
        steps.retain(|s| s["step"] != name);
        steps.push(json!({ "step": name, "ok": ok, "detail": self.redact(id, &detail) }));
        if let Ok(mut c) = self.connection(id) {
            c["check"] = json!(steps);
            let _ = self.db.put_config(CONNECTION, id, &c, now_ms());
        }
    }

    /// What the chosen auth method needs before the agent can start, checked without launching it.
    pub(super) fn credential_check(&self, c: &Value) -> (bool, String) {
        let id = str_of(c, "id");
        let home = self.agent_dir(id).join("home");
        let secrets = self.secrets(id);
        let has = |k: &str| secrets[k].as_str().is_some_and(|v| !v.is_empty());
        let desc = kinds().as_array().unwrap().iter().find(|k| k["id"] == c["kind"]).cloned().unwrap_or(Value::Null);
        if !desc["token"].is_null() {
            return if has("token") {
                (true, format!("已保存 {}", desc["token"]["label"].as_str().unwrap_or("token")))
            } else {
                (false, format!("没有填写 {}", desc["token"]["label"].as_str().unwrap_or("token")))
            };
        }
        match (str_of(c, "base"), str_of(c, "auth")) {
            (_, "import") => {
                let files: Vec<String> = std::fs::read_dir(&home)
                    .map(|d| {
                        d.flatten()
                            .map(|e| e.file_name().to_string_lossy().to_string())
                            .filter(|n| {
                                matches!(
                                    n.as_str(),
                                    "config.toml"
                                        | "auth.json"
                                        | "settings.json"
                                        | "models.json"
                                        | ".credentials.json"
                                        | ".claude.json"
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let env: Vec<String> =
                    secrets["importedEnv"].as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
                if files.is_empty() && env.is_empty() {
                    (false, "没有复制到任何配置，请检查来源目录后重新保存".into())
                } else {
                    (
                        true,
                        [files.join("、"), env.iter().map(|e| format!("${e}")).collect::<Vec<_>>().join("、")]
                            .into_iter()
                            .filter(|s| !s.is_empty())
                            .collect::<Vec<_>>()
                            .join("，"),
                    )
                }
            }
            (_, "chatgpt") | (_, "account") => {
                if home.join("auth.json").exists() {
                    (true, "已登录".into())
                } else {
                    (false, "还没有登录".into())
                }
            }
            (_, "apikey") => {
                if has("apiKey") {
                    (true, "已保存 API Key".into())
                } else {
                    (false, "没有填写 API Key".into())
                }
            }
            (_, "oauth") => {
                if has("token") {
                    (true, "已保存 OAuth Token".into())
                } else {
                    (false, "没有填写 OAuth Token".into())
                }
            }
            ("custom", _) => (true, "由启动命令自己负责".into()),
            _ => (true, "无需凭据".into()),
        }
    }

    /// The full configuration check: runtime → credentials → ACP handshake → session and models → one real reply.
    pub async fn test_connection(self: &Arc<Self>, id: &str) -> Result<Value> {
        let dir = self.agent_dir(id);
        let probe = dir.join("probe");
        std::fs::create_dir_all(&probe)?;
        let log = dir.join("test.log");
        let _ = std::fs::remove_file(&log);
        let started = now_ms();
        let mut steps: Vec<Value> = vec![];
        let c0 = self.connection(id)?;

        let outcome: Result<Value> = async {
            // An agent's adapter is installed the first time it is needed: here, before the check uses it.
            if let Some(adapter) = crate::adapters::for_base(str_of(&c0, "base")) {
                self.check_step(id, &mut steps, "ACP 适配器", None, format!("检查 {} 的适配器…", adapter.name));
                let app = self.clone();
                let installed = tokio::task::spawn_blocking(move || app.ensure_adapter(adapter.id)).await?;
                match installed {
                    Ok(Some(v)) => self.check_step(id, &mut steps, "ACP 适配器", Some(true), format!("已安装 {} {v}", adapter.package)),
                    Ok(None) => {
                        let v = self.adapter_version(adapter.id).unwrap_or_default();
                        self.check_step(id, &mut steps, "ACP 适配器", Some(true), format!("{} {v}", adapter.package));
                    }
                    Err(err) => {
                        self.check_step(id, &mut steps, "ACP 适配器", Some(false), format!("安装失败：{err:#}"));
                        return Err(err);
                    }
                }
            }
            let spec = match self.spec_for(id) {
                Ok(spec) => spec,
                Err(err) => {
                    self.check_step(id, &mut steps, "运行时", Some(false), format!("{err:#}"));
                    return Err(err);
                }
            };
            let program = spec.args.first().cloned().unwrap_or_else(|| spec.program.clone());
            self.check_step(id, &mut steps, "运行时", Some(true), program);

            let (cred_ok, cred) = self.credential_check(&c0);
            self.check_step(id, &mut steps, "凭据", Some(cred_ok), cred.clone());
            if !cred_ok {
                bail!("{cred}");
            }

            self.check_step(id, &mut steps, "启动 Agent", None, "通过 ACP 握手…".into());
            let (conn, mut rx) = match tokio::time::timeout(Duration::from_secs(60), AcpConn::spawn(&spec, &probe, &log)).await {
                Ok(Ok(x)) => x,
                Ok(Err(err)) => {
                    self.check_step(id, &mut steps, "启动 Agent", Some(false), format!("{err:#}"));
                    return Err(err);
                }
                Err(_) => {
                    self.check_step(id, &mut steps, "启动 Agent", Some(false), "60 秒内没有完成 ACP 握手".into());
                    bail!("60 秒内没有完成 ACP 握手");
                }
            };
            self.check_step(id, &mut steps, "启动 Agent", Some(true), format!("{} {}", conn.agent_info["name"].as_str().unwrap_or("agent"), conn.agent_info["version"].as_str().unwrap_or("")));

            let result: Result<Value> = async {
                self.check_step(id, &mut steps, "打开会话", None, "读取模型列表…".into());
                let session = match tokio::time::timeout(Duration::from_secs(60), conn.request("session/new", json!({ "cwd": probe, "mcpServers": [], "_meta": crate::orchestrator::session_meta() }))).await {
                    Ok(r) => r,
                    Err(_) => Err(anyhow!("60 秒内没有打开会话")),
                };
                let session = match session {
                    Ok(s) => s,
                    Err(err) => {
                        self.check_step(id, &mut steps, "打开会话", Some(false), auth_hint(&format!("{err:#}")));
                        return Err(err);
                    }
                };
                let sid = session["sessionId"].as_str().context("session/new 没有返回 sessionId")?.to_string();
                let options = session_options(&session);
                let models = options["model"]["options"].as_array().map(|a| a.len()).unwrap_or(0);
                let perms: Vec<String> = options["controls"].as_array().cloned().unwrap_or_default().iter().filter(|c| c["permission"] == true).map(|c| match c["options"].as_array() {
                    Some(o) if !o.is_empty() => format!("{}：{}", str_of(c, "name"), o.iter().map(|x| str_of(x, "name").to_string()).collect::<Vec<_>>().join(" / ")),
                    _ => format!("{}（开关）", str_of(c, "name")),
                }).collect();
                let models = if models > 0 { format!("{models} 个模型，默认 {}", options["model"]["current"].as_str().unwrap_or("")) } else { "Agent 没有提供模型列表".into() };
                self.check_step(id, &mut steps, "打开会话", Some(true), if perms.is_empty() { format!("{models}；没有声明权限模式") } else { format!("{models}；{}", perms.join("；")) });

                self.check_step(id, &mut steps, "试运行", None, "发送一句测试消息…".into());
                let prompt = conn.request("session/prompt", json!({ "sessionId": sid, "prompt": [{ "type": "text", "text": "Reply with exactly the word PONG and nothing else." }] }));
                tokio::pin!(prompt);
                let mut reply = String::new();
                let deadline = tokio::time::sleep(Duration::from_secs(120));
                tokio::pin!(deadline);
                let run: Result<()> = loop {
                    tokio::select! {
                        r = &mut prompt => break r.map(|_| ()),
                        _ = &mut deadline => break Err(anyhow!("120 秒内没有回复")),
                        Some(msg) = rx.recv() => match msg {
                            Incoming::Notification { params, .. } => {
                                let u = &params["update"];
                                if u["sessionUpdate"] == "agent_message_chunk" {
                                    reply.push_str(u["content"]["text"].as_str().unwrap_or(""));
                                }
                            }
                            Incoming::Request { id, method, .. } => { let _ = conn.respond(id, crate::orchestrator::cancelled_reply(&method)).await; }
                            Incoming::Closed => break Err(anyhow!("Agent 进程已退出")),
                        }
                    }
                };
                if let Err(err) = run {
                    self.check_step(id, &mut steps, "试运行", Some(false), auth_hint(&format!("{err:#}")));
                    return Err(err);
                }
                if reply.trim().is_empty() {
                    self.check_step(id, &mut steps, "试运行", Some(false), "Agent 没有返回任何内容，可能是模型或凭据有误".into());
                    bail!("Agent 没有返回任何内容");
                }
                self.check_step(id, &mut steps, "试运行", Some(true), format!("回复：{}", crate::orchestrator::short(reply.trim(), 40)));
                Ok(json!({ "agentInfo": conn.agent_info, "options": options, "reply": reply.trim() }))
            }
            .await;
            conn.kill().await;
            result
        }
        .await;

        let mut c = self.connection(id)?;
        c["checkedAt"] = json!(now_ms());
        c["elapsedMs"] = json!(now_ms() - started);
        c["check"] = json!(steps);
        match outcome {
            Ok(found) => {
                c["status"] = json!("ok");
                c["error"] = Value::Null;
                c["errorLog"] = Value::Null;
                c["agentInfo"] = found["agentInfo"].clone();
                c["options"] = found["options"].clone();
                c["reply"] = json!(crate::orchestrator::short(found["reply"].as_str().unwrap_or(""), 80));
            }
            Err(err) => {
                let failed = steps
                    .iter()
                    .find(|s| s["ok"] == false)
                    .map(|s| format!("{}：{}", str_of(s, "step"), str_of(s, "detail")))
                    .unwrap_or_else(|| auth_hint(&format!("{err:#}")));
                c["status"] = json!("error");
                c["error"] = json!(self.redact(id, &failed));
                c["errorLog"] = json!(self.redact(id, &tail(&log, 15)));
            }
        }
        self.db.put_config(CONNECTION, id, &c, now_ms())?;
        Ok(c)
    }

    /// Codex ChatGPT login through the adapter's bundled CLI, into the connection's own
    /// CODEX_HOME. `codex login` opens the browser itself.
    pub fn start_login(self: &Arc<Self>, id: &str) -> Result<()> {
        let mut c = self.connection(id)?;
        if !account_login(&c) {
            bail!("这个 Provider 的认证方式不需要登录");
        }
        if c["login"]["state"] == "running" {
            return Ok(());
        }
        let spec = self.spec_for(id)?;
        let entry = spec.args.first().context("ACP 适配器没有入口文件")?;
        let dir = self.agent_dir(id);
        let log = std::fs::File::create(dir.join("login.log"))?;
        let mut cmd = tokio::process::Command::new(&spec.program);
        cmd.env_clear();
        let mut child = cmd
            .envs(spec.env.iter().cloned())
            .arg(entry)
            .args(["cli", "login"])
            .env("CODEX_HOME", dir.join("home"))
            .stdin(std::process::Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .kill_on_drop(true)
            .spawn()
            .context("启动登录命令失败")?;
        c["login"] = json!({ "state": "running", "at": now_ms() });
        self.db.put_config(CONNECTION, id, &c, now_ms())?;
        let app = self.clone();
        let id = id.to_string();
        tokio::spawn(async move {
            let status = tokio::time::timeout(Duration::from_secs(600), child.wait()).await;
            let state = match status {
                Ok(Ok(s)) if s.success() => "done",
                Ok(_) => "failed",
                Err(_) => {
                    let _ = child.kill().await;
                    "timeout"
                }
            };
            if let Ok(mut c) = app.connection(&id) {
                c["login"] = json!({ "state": state, "at": now_ms() });
                c["status"] = json!("untested");
                let _ = app.db.put_config(CONNECTION, &id, &c, now_ms());
            }
        });
        Ok(())
    }
}
