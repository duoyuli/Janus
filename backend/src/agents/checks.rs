//! Checking a connection step by step (adapter, runtime, credentials, a session and a trial prompt)
//! and signing in to an agent account.

use super::*;
use std::sync::Mutex as StdMutex;

impl App {
    /// Start a configuration check in the background, so it finishes even if the app that asked
    /// goes away. Asking again while it runs returns the running check.
    pub fn start_check(self: &Arc<Self>, id: &str) -> Result<Value> {
        let mut running = self.checks.0.lock().unwrap();
        let mut c = self.connection(id)?;
        if let ConnCheck::Testing { started } = check_state(&c)
            && running.get(id) == Some(&started)
        {
            return Ok(c);
        }
        let started = now_ms();
        set_check(&mut c, ConnCheck::Testing { started });
        c["check"] = json!([]);
        self.db.put_config(CONNECTION, id, &c, now_ms())?;
        running.insert(id.to_string(), started);
        drop(running);
        let app = self.clone();
        let id = id.to_string();
        tokio::spawn(async move {
            let _ended = CheckEnded { app: app.clone(), id: id.clone(), started };
            if let Err(err) = app.test_connection(&id, started).await {
                crate::config::log(format!("janus: check {id} failed to record: {err:#}"));
            }
        });
        Ok(c)
    }

    /// A check or a login left half-done by a restart: the task that would have recorded its end
    /// went with the old process (and `kill_on_drop` took the login command with it). The check
    /// reads as "not tested" again and the login as failed, so the next login starts a new one.
    pub fn reset_stale_checks(&self) -> Result<()> {
        for mut c in self.db.list_config(CONNECTION)? {
            let testing = matches!(check_state(&c), ConnCheck::Testing { .. });
            let logging_in = login_state(&c) == LoginState::Running;
            if !testing && !logging_in {
                continue;
            }
            if testing {
                set_check(&mut c, ConnCheck::Untested);
            }
            if logging_in {
                set_login(&mut c, LoginState::Failed);
            }
            self.db.put_config(CONNECTION, str_of(&c, "id"), &c, now_ms())?;
        }
        Ok(())
    }

    /// Record one step of the check so the app can show progress while it runs.
    fn check_step(&self, id: &str, check: &mut Check, name: &str, ok: Option<bool>, detail: String) {
        check.steps.retain(|s| s["step"] != name);
        check.steps.push(json!({ "step": name, "ok": ok, "detail": self.redact(id, &detail) }));
        if let Ok(mut c) = self.connection(id)
            && check.owns(&c)
        {
            c["check"] = json!(check.steps);
            let _ = self.db.put_config(CONNECTION, id, &c, now_ms());
        }
    }

    /// What the chosen auth method needs before the agent can start, checked without launching it.
    pub(super) fn credential_check(&self, c: &Value) -> (bool, String) {
        let id = str_of(c, "id");
        let home = self.agent_config(id);
        let secrets = match super::environment::read_secrets(&self.cfg.layout.agent_secrets(id)) {
            Ok(s) => s,
            Err(_) => return (false, "无法读取 Agent 凭据文件".into()),
        };
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
            // Claude's custom API is its environment; a gateway may name its credential anything,
            // so only the trial run can tell whether it works.
            ("claude", "apikey") => (true, "由 Claude Code 在试运行时验证环境变量".into()),
            ("codex", "apikey") => {
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
            _ => (true, "无需凭据".into()),
        }
    }

    /// The full configuration check: runtime → credentials → ACP handshake → session and models →
    /// one real reply. A check that a newer one or a save of the connection replaced records nothing.
    async fn test_connection(self: &Arc<Self>, id: &str, started: i64) -> Result<()> {
        // The session the check opens works in an empty folder of its own.
        let probe = self.cfg.layout.agent_tmp(id).join("probe");
        std::fs::create_dir_all(&probe)?;
        let logs = self.cfg.layout.agent_logs(id);
        std::fs::create_dir_all(&logs)?;
        let log = logs.join("test.log");
        let _ = std::fs::remove_file(&log);
        let mut check = Check { started, steps: vec![] };
        let c0 = self.connection(id)?;

        let outcome: Result<Value> = async {
            // An agent's adapter is installed the first time it is needed: here, before the check uses it.
            if let Some(adapter) = crate::adapters::for_base(str_of(&c0, "base")) {
                self.check_step(id, &mut check, "ACP 适配器", None, format!("检查 {} 的适配器…", adapter.name));
                let app = self.clone();
                let installed = tokio::task::spawn_blocking(move || app.ensure_adapter(adapter.id)).await?;
                match installed {
                    Ok(Some(v)) => self.check_step(id, &mut check, "ACP 适配器", Some(true), format!("已安装 {} {v}", adapter.package)),
                    Ok(None) => {
                        let v = self.adapter_version(adapter.id).unwrap_or_default();
                        self.check_step(id, &mut check, "ACP 适配器", Some(true), format!("{} {v}", adapter.package));
                    }
                    Err(err) => {
                        self.check_step(id, &mut check, "ACP 适配器", Some(false), format!("安装失败：{err:#}"));
                        return Err(err);
                    }
                }
            }
            let spec = match self.spec_for(id) {
                Ok(spec) => spec,
                Err(err) => {
                    self.check_step(id, &mut check, "运行时", Some(false), format!("{err:#}"));
                    return Err(err);
                }
            };
            let program = spec.args.first().cloned().unwrap_or_else(|| spec.program.clone());
            self.check_step(id, &mut check, "运行时", Some(true), program);

            let (cred_ok, cred) = self.credential_check(&c0);
            self.check_step(id, &mut check, "凭据", Some(cred_ok), cred.clone());
            if !cred_ok {
                bail!("{cred}");
            }

            self.check_step(id, &mut check, "启动 Agent", None, "通过 ACP 握手…".into());
            let (conn, mut rx) = match tokio::time::timeout(Duration::from_secs(60), AcpConn::spawn(&spec, &probe, || crate::acp::open_log(&log))).await {
                Ok(Ok(x)) => x,
                Ok(Err(err)) => {
                    self.check_step(id, &mut check, "启动 Agent", Some(false), format!("{err:#}"));
                    return Err(err);
                }
                Err(_) => {
                    self.check_step(id, &mut check, "启动 Agent", Some(false), "60 秒内没有完成 ACP 握手".into());
                    bail!("60 秒内没有完成 ACP 握手");
                }
            };
            self.check_step(id, &mut check, "启动 Agent", Some(true), format!("{} {}", conn.agent_info["name"].as_str().unwrap_or("agent"), conn.agent_info["version"].as_str().unwrap_or("")));

            let result: Result<Value> = async {
                self.check_step(id, &mut check, "打开会话", None, "读取模型列表…".into());
                let session = match tokio::time::timeout(Duration::from_secs(60), conn.request("session/new", json!({ "cwd": probe, "mcpServers": [] }))).await {
                    Ok(r) => r,
                    Err(_) => Err(anyhow!("60 秒内没有打开会话")),
                };
                let session = match session {
                    Ok(s) => s,
                    Err(err) => {
                        self.check_step(id, &mut check, "打开会话", Some(false), auth_hint(&format!("{err:#}")));
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
                self.check_step(id, &mut check, "打开会话", Some(true), if perms.is_empty() { format!("{models}；没有声明权限模式") } else { format!("{models}；{}", perms.join("；")) });

                self.check_step(id, &mut check, "试运行", None, "发送一句测试消息…".into());
                let prompt = conn.request("session/prompt", json!({ "sessionId": sid, "prompt": [{ "type": "text", "text": "Reply with exactly the word PONG and nothing else." }] }));
                tokio::pin!(prompt);
                let mut reply = String::new();
                let deadline = tokio::time::sleep(Duration::from_secs(120));
                tokio::pin!(deadline);
                let run: Result<()> = loop {
                    // Messages first: the agent's reply arrives before its prompt response, and a
                    // fast agent has both ready at once; picking the response first lost the reply.
                    tokio::select! {
                        biased;
                        Some(msg) = rx.recv() => match msg {
                            Incoming::Notification { params, .. } => {
                                let u = &params["update"];
                                if u["sessionUpdate"] == "agent_message_chunk" {
                                    reply.push_str(u["content"]["text"].as_str().unwrap_or(""));
                                }
                            }
                            Incoming::Request { id, method, .. } => { let _ = conn.respond(id, crate::orchestrator::cancelled_reply(&method)).await; }
                            Incoming::Closed => break Err(anyhow!("Agent 进程已退出")),
                        },
                        r = &mut prompt => break r.map(|_| ()),
                        _ = &mut deadline => break Err(anyhow!("120 秒内没有回复")),
                    }
                };
                if let Err(err) = run {
                    self.check_step(id, &mut check, "试运行", Some(false), auth_hint(&format!("{err:#}")));
                    return Err(err);
                }
                if reply.trim().is_empty() {
                    self.check_step(id, &mut check, "试运行", Some(false), "Agent 没有返回任何内容，可能是模型或凭据有误".into());
                    bail!("Agent 没有返回任何内容");
                }
                self.check_step(id, &mut check, "试运行", Some(true), format!("回复：{}", crate::orchestrator::short(reply.trim(), 40)));
                Ok(json!({ "agentInfo": conn.agent_info, "options": options, "reply": reply.trim() }))
            }
            .await;
            conn.kill().await;
            result
        }
        .await;

        let mut c = self.connection(id)?;
        if !check.owns(&c) {
            return Ok(());
        }
        c["checkedAt"] = json!(now_ms());
        c["elapsedMs"] = json!(now_ms() - started);
        c["check"] = json!(check.steps);
        match outcome {
            Ok(found) => {
                set_check(&mut c, ConnCheck::Ok);
                c["agentInfo"] = found["agentInfo"].clone();
                c["options"] = found["options"].clone();
                c["reply"] =
                    json!(crate::orchestrator::short(&self.redact(id, found["reply"].as_str().unwrap_or("")), 80));
            }
            Err(err) => {
                let failed = check
                    .steps
                    .iter()
                    .find(|s| s["ok"] == false)
                    .map(|s| format!("{}：{}", str_of(s, "step"), str_of(s, "detail")))
                    .unwrap_or_else(|| auth_hint(&format!("{err:#}")));
                set_check(
                    &mut c,
                    ConnCheck::Error { error: self.redact(id, &failed), log: self.redact(id, &tail(&log, 15)) },
                );
            }
        }
        self.db.put_config(CONNECTION, id, &c, now_ms())?;
        Ok(())
    }

    /// Codex ChatGPT login through the adapter's bundled CLI, into the connection's own
    /// CODEX_HOME. `codex login` opens the browser itself.
    pub fn start_login(self: &Arc<Self>, id: &str) -> Result<()> {
        let mut c = self.connection(id)?;
        if !account_login(&c) {
            bail!("这个 Provider 的认证方式不需要登录");
        }
        if login_state(&c) == LoginState::Running {
            return Ok(());
        }
        let spec = self.spec_for(id)?;
        let entry = spec.args.first().context("ACP 适配器没有入口文件")?;
        let logs = self.cfg.layout.agent_logs(id);
        std::fs::create_dir_all(&logs)?;
        let log = std::fs::File::create(logs.join("login.log"))?;
        let mut cmd = tokio::process::Command::new(&spec.program);
        cmd.env_clear();
        let mut child = cmd
            .envs(spec.env.iter().cloned())
            .arg(entry)
            .args(["cli", "login"])
            .env("CODEX_HOME", self.agent_config(id))
            .stdin(std::process::Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .kill_on_drop(true)
            .spawn()
            .context("启动登录命令失败")?;
        set_login(&mut c, LoginState::Running);
        self.db.put_config(CONNECTION, id, &c, now_ms())?;
        let app = self.clone();
        let id = id.to_string();
        tokio::spawn(async move {
            let status = tokio::time::timeout(Duration::from_secs(600), child.wait()).await;
            let state = match status {
                Ok(Ok(s)) if s.success() => LoginState::Done,
                Ok(_) => LoginState::Failed,
                Err(_) => {
                    let _ = child.kill().await;
                    LoginState::Timeout
                }
            };
            if let Ok(mut c) = app.connection(&id) {
                set_login(&mut c, state);
                set_check(&mut c, ConnCheck::Untested);
                let _ = app.db.put_config(CONNECTION, &id, &c, now_ms());
            }
        });
        Ok(())
    }
}

/// The configuration checks running in this process: when each connection's started. A record
/// that reads `testing` with another start time was cut off (a panic in the check), so asking
/// again starts a new one instead of returning it.
#[derive(Default)]
pub struct CheckState(StdMutex<HashMap<String, i64>>);

/// One running check: its steps so far, and when it started, which tells whether the stored
/// record is still its own. Saving the connection writes a record without that start time, and
/// a newer check writes its own: either way this one stops recording.
struct Check {
    started: i64,
    steps: Vec<Value>,
}

impl Check {
    fn owns(&self, c: &Value) -> bool {
        c["testStartedAt"].as_i64() == Some(self.started)
    }
}

/// Takes a check off the running list when it ends, also when it panics.
struct CheckEnded {
    app: Arc<App>,
    id: String,
    started: i64,
}

impl Drop for CheckEnded {
    fn drop(&mut self) {
        if let Ok(mut running) = self.app.checks.0.lock()
            && running.get(&self.id) == Some(&self.started)
        {
            running.remove(&self.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrator::test_app;

    /// A restart in the middle of a check or a login leaves neither stuck: the connection no
    /// longer reads as testing or logging in, and the next login is not taken for one running.
    #[test]
    fn a_restart_ends_a_check_or_login_left_running() {
        let dir = std::env::temp_dir().join(format!("janus-stale-checks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let app = test_app(&dir);
        let login = json!({ "auth": "chatgpt", "login": { "state": "running", "at": 1 } });
        let check = json!({ "status": "testing", "testStartedAt": 1 });
        let settled = json!({ "status": "ok", "login": { "state": "done", "at": 1 } });
        for (id, extra) in [("logging-in", login), ("testing", check), ("settled", settled)] {
            let c = crate::agents::test_connection(id, "codex", "codex", extra);
            app.db.put_config(CONNECTION, id, &c, 1).unwrap();
        }

        app.reset_stale_checks().unwrap();

        let all = app.connections().unwrap();
        let get = |id: &str| all.iter().find(|c| c["id"] == id).unwrap().clone();
        let logging_in = get("logging-in");
        assert_eq!(logging_in["login"]["state"], "failed");
        assert_eq!(logging_in["phase"], "logged_out", "the card offers to log in again");
        assert_eq!(get("testing")["status"], "untested");
        let settled = get("settled");
        assert_eq!((settled["status"].as_str(), settled["login"]["at"].as_i64()), (Some("ok"), Some(1)), "left alone");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Asking for a check while one runs returns it; once it is no longer running (it panicked),
    /// asking starts a new one. A check that a save or a newer check replaced records nothing.
    #[tokio::test]
    async fn only_the_latest_check_of_a_connection_records_its_result() {
        let dir = std::env::temp_dir().join(format!("janus-check-owner-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let app = test_app(&dir);
        // An agent that never answers the handshake keeps its check running: the scripted agent,
        // told to stay mute.
        let fake = dir.join("fake");
        std::fs::create_dir_all(&fake).unwrap();
        std::fs::write(fake.join("mute"), "").unwrap();
        let slow = crate::agents::test_connection(
            "slow",
            "claude",
            "claude",
            json!({ "env": { "FAKE_DIR": fake.to_string_lossy() } }),
        );
        app.db.put_config(CONNECTION, "slow", &slow, 1).unwrap();
        let first = app.start_check("slow").unwrap();
        let again = app.start_check("slow").unwrap();
        assert_eq!(again["testStartedAt"], first["testStartedAt"], "the running check is returned");
        app.checks.0.lock().unwrap().clear();
        std::thread::sleep(Duration::from_millis(2));
        let fresh = app.start_check("slow").unwrap();
        assert_ne!(fresh["testStartedAt"], first["testStartedAt"], "a check no longer running is started anew");

        // An agent that exits at once (the scripted agent without its FAKE_DIR) ends its check
        // quickly; the record now belongs to another.
        let gone = crate::agents::test_connection(
            "gone",
            "claude",
            "claude",
            json!({ "status": "testing", "testStartedAt": 2, "check": [] }),
        );
        app.db.put_config(CONNECTION, "gone", &gone, 1).unwrap();
        app.test_connection("gone", 1).await.unwrap();
        let after = app.connection("gone").unwrap();
        assert_eq!((after["status"].as_str(), after["check"].clone()), (Some("testing"), json!([])), "untouched");
        app.test_connection("gone", 2).await.unwrap();
        assert_eq!(app.connection("gone").unwrap()["status"], "error", "its own check records the result");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A check's state is written whole: running has its start, failing has its reason, and a
    /// pass clears the reason of an earlier failure.
    #[test]
    fn a_check_state_is_written_whole() {
        let mut c = json!({ "id": "x" });
        set_check(&mut c, ConnCheck::Testing { started: 5 });
        assert_eq!(check_state(&c), ConnCheck::Testing { started: 5 });
        set_check(&mut c, ConnCheck::Error { error: "凭据：401".into(), log: "tail".into() });
        assert_eq!(connection_phase(&c), "error");
        set_check(&mut c, ConnCheck::Ok);
        assert!(c["error"].is_null() && c["errorLog"].is_null(), "no old reason after a pass: {c}");
        assert_eq!(c["testStartedAt"], 5, "the latest check's start stays, telling late writes apart");
        set_login(&mut c, LoginState::Running);
        assert_eq!((login_state(&c), connection_phase(&c)), (LoginState::Running, "logging_in"));
        set_login(&mut c, LoginState::Timeout);
        assert_eq!(login_state(&c), LoginState::Timeout);
    }
}
