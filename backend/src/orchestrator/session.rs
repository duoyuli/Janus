//! Opening an ACP session for a member: MCP servers, the system prompt, and the controls the role
//! asks for.

use super::*;

/// What a session opens with besides its working directory: the member's resources the way its
/// agent takes them, Janus's own servers (the board), and the system layer. A reloaded session
/// gets the same, so it keeps its skills, servers and instructions.
// @project-doc docs/interfaces/20261031-acp_integration.md#session_meta
pub(super) struct Opening {
    pub mcp_servers: Vec<Value>,
    pub meta: Value,
    /// Each of the member's resources and how it reached the session, for the record.
    pub resources: Vec<crate::library::Delivery>,
}

impl Opening {
    pub(super) fn new_params(&self, cwd: &Path) -> Value {
        json!({ "cwd": cwd, "mcpServers": self.mcp_servers, "_meta": self.meta })
    }

    pub(super) fn load_params(&self, session: &str, cwd: &Path) -> Value {
        json!({ "sessionId": session, "cwd": cwd, "mcpServers": self.mcp_servers, "_meta": self.meta })
    }
}

impl App {
    /// Start the agent of a session and make it live: its connection is registered (cancelling the
    /// run kills it), the person's commands reach it, its agent can be found by name, and Janus
    /// pushes into it when it takes steering. `who` names it in the timeout message. Everything
    /// here is taken back by `close_session`.
    /// Its process takes a seat first (`take_seat`): a collaborator's waits in line as the team's,
    /// the lead's as its run's (`Run::asker`).
    pub(super) async fn open_session(
        &self,
        run: &Run,
        owner: &Owner,
        agent: &str,
        spec: &crate::acp::RuntimeSpec,
        log: &Path,
        who: &str,
    ) -> Result<(Arc<AcpConn>, mpsc::UnboundedReceiver<Incoming>, Control)> {
        let asker = match owner.session {
            SessionKey::Task(_) => Asker::Team,
            SessionKey::Lead => run.asker,
        };
        let seat = self.take_seat(run, owner, agent, asker).await?;
        // The log goes in the run's folders: never once `delete_run` has marked the run (`Run.files`).
        let open = || {
            let _files = run.files.read().unwrap_or_else(|p| p.into_inner());
            if *run.emitting.lock().unwrap_or_else(|p| p.into_inner()) {
                return Err(Cancelled.into());
            }
            crate::acp::open_log(log)
        };
        let (conn, rx) = tokio::time::timeout(STARTUP_TIMEOUT, AcpConn::spawn(spec, &run.cwd, open))
            .await
            .map_err(|_| anyhow!("{who}启动超过 {} 分钟没有完成", STARTUP_TIMEOUT.as_secs() / 60))??;
        conn.hold(seat);
        let key = &owner.session;
        let (tx, steer_rx) = mpsc::unbounded_channel();
        let cancelled = {
            let mut st = run.st.lock().unwrap();
            // A run being deleted (`delete_run` marks it, then stops what is registered) takes no
            // new agent either: checked while registering, so none slips in between the two.
            let deleted = *run.emitting.lock().unwrap_or_else(|p| p.into_inner());
            if !st.cancelled() && !deleted {
                st.conns.insert(key.clone(), conn.clone());
                st.live.insert(key.clone(), tx);
                st.live_agent.insert(agent.to_string(), key.clone());
                if conn.steering {
                    st.steerable.insert(key.clone());
                }
            }
            st.cancelled() || deleted
        };
        // Cancelling (or deleting) stops every registered agent; one that started meanwhile stops here.
        if cancelled {
            conn.kill().await;
            return Err(Cancelled.into());
        }
        let ctl = Control::new(steer_rx, conn.steering, owner.clone());
        Ok((conn, rx, ctl))
    }

    /// The session is open and working: the watchdog follows it from now on.
    pub(super) fn watch_session(&self, run: &Run, key: &SessionKey, attempt: i64) {
        let now = now_ms();
        run.watch.lock().unwrap().insert(
            key.clone(),
            Watch { attempt, started_ms: now, last_ms: now, phase: AgentPhase::Thinking, ..Default::default() },
        );
    }

    /// Take back what `open_session` and `watch_session` set up, withdraw the decisions the
    /// session still waited on, and stop its agent. Entries a newer session of the same key put
    /// in place stay.
    pub(super) async fn close_session(&self, run: &Run, owner: &Owner, conn: Option<&Arc<AcpConn>>, why: &str) {
        let key = &owner.session;
        {
            let mut watch = run.watch.lock().unwrap();
            if watch.get(key).is_some_and(|w| w.attempt == owner.attempt) {
                watch.remove(key);
            }
        }
        let stopped = {
            let mut st = run.st.lock().unwrap();
            let mine = match (st.conns.get(key), conn) {
                (Some(registered), Some(conn)) => Arc::ptr_eq(registered, conn),
                _ => true,
            };
            if mine {
                st.live.remove(key);
                st.live_agent.retain(|_, k| k != key);
                st.steerable.remove(key);
                st.conns.remove(key)
            } else {
                None
            }
        };
        self.withdraw_owned(run, owner, why);
        // The session's library servers go with it, unless a newer session of the same owner
        // already took its place (and uses them).
        // @project-doc docs/domains/20261116-resource_library.md#mcp_router
        if stopped.is_some() || conn.is_none() {
            run.router.stop_session(match key {
                SessionKey::Lead => "lead",
                SessionKey::Task(id) => id,
            });
        }
        if let Some(conn) = stopped.or_else(|| conn.cloned()) {
            conn.kill().await;
        }
    }
}

/// Whether the agent declared image input (`promptCapabilities.image`) when it started.
pub(super) fn takes_images(conn: &AcpConn) -> bool {
    conn.capabilities["promptCapabilities"]["image"] == true
}

/// Swallow the history a `session/load` replays. Updates may trail the response, so wait until
/// the agent has been quiet for a moment; anything it asks meanwhile is declined.
pub(super) async fn drain_replay(conn: &AcpConn, rx: &mut mpsc::UnboundedReceiver<Incoming>) {
    let quiet = Duration::from_millis(500);
    let until = tokio::time::Instant::now() + Duration::from_secs(10);
    while tokio::time::Instant::now() < until {
        match tokio::time::timeout(quiet, rx.recv()).await {
            Ok(Some(Incoming::Notification { .. })) => {}
            Ok(Some(Incoming::Request { id, .. })) => {
                let _ = conn.respond_error(id, "session is still loading").await;
            }
            Ok(Some(Incoming::Closed)) | Ok(None) | Err(_) => break,
        }
    }
}

pub(super) async fn new_session(conn: &AcpConn, cwd: &Path, opening: &Opening) -> Result<Value> {
    let r = conn.request("session/new", opening.new_params(cwd)).await?;
    r["sessionId"].as_str().context("session/new 没有返回 sessionId")?;
    Ok(r)
}

impl App {
    /// The opening of a member's session. `caps` are what its agent declared in `initialize`;
    /// `system` is the system layer: Janus's instructions for this session, then the person's
    /// standing rules. Claude Code takes it in `_meta.systemPrompt.append`; Codex has it in its
    /// process environment already (`with_standing`).
    pub(super) fn opening(
        &self,
        run: &Run,
        who: &str,
        m: &Member,
        caps: &Value,
        extra: Vec<Value>,
        system: Option<&str>,
    ) -> Opening {
        let bundle = self.dirs(run).bundle(&m.id);
        let launch = crate::library::launch(m.runtime, &m.resources, caps, &bundle, self.tools_server(run, who));
        let mut meta = launch.meta;
        m.runtime.open_meta(&mut meta, system);
        let mut mcp_servers = launch.mcp_servers;
        mcp_servers.extend(extra);
        Opening { mcp_servers, meta, resources: launch.delivered }
    }
}

/// The control values a session of this member opens with: exactly what its role chose. Read
/// tasks get no override; they run in a disposable snapshot whatever the mode.
pub(super) fn session_values(m: &Member) -> Value {
    match &m.session {
        Value::Object(o) => Value::Object(o.clone()),
        _ => json!({}),
    }
}

/// Apply the role's model, effort and session controls (permission mode and whatever else its
/// agent declares). Returns what was applied, so the attempt records the exact settings it ran with.
// @project-doc docs/interfaces/20261031-acp_integration.md#session_setup
pub(super) async fn configure_session(conn: &AcpConn, session: &Value, m: &Member) -> Result<Value> {
    let sid = session["sessionId"].as_str().unwrap_or_default();
    let mut opts = crate::agents::session_options(session);
    let mut applied = json!({ "controls": {} });
    for (cat, want, required) in [("model", m.model.as_str(), true), ("effort", m.effort.as_str(), false)] {
        let opt = opts[cat].clone();
        if opt.is_null() {
            continue;
        }
        let current = opt["current"].as_str().unwrap_or_default();
        if want.is_empty() || want == current {
            applied[cat] = json!(current);
            continue;
        }
        if !opt["options"].as_array().is_some_and(|o| o.iter().any(|x| x["value"] == want)) {
            if required {
                bail!("{} 不提供模型 {want}，请在数字员工设置中重新选择", m.connection_name);
            }
            applied[cat] = json!(current);
            continue;
        }
        match conn
            .request("session/set_config_option", json!({ "sessionId": sid, "configId": opt["id"], "value": want }))
            .await
        {
            Ok(r) => {
                applied[cat] = json!(want);
                // A model switch can change what else is on offer (Claude Code drops Auto on some models).
                if r["configOptions"].is_array() {
                    opts = crate::agents::session_options(&r);
                }
            }
            Err(err) if required => return Err(err.context(format!("设置模型 {want} 失败"))),
            Err(_) => applied[cat] = json!(current),
        }
    }
    let wants = session_values(m);
    for c in opts["controls"].as_array().cloned().unwrap_or_default() {
        let id = c["id"].as_str().unwrap_or_default();
        let current = c["current"].clone();
        let mut want = wants.get(id).cloned().unwrap_or(Value::Null);
        // A session that starts in a plan mode (the agent's own default) moves to the first other option.
        if want.is_null() && crate::agents::is_plan_value(&current) {
            want = c["options"][0]["value"].clone();
        }
        let offered = match &want {
            Value::Bool(_) => c["type"] == "boolean",
            Value::String(_) => c["options"].as_array().is_some_and(|o| o.iter().any(|x| x["value"] == want)),
            _ => false,
        };
        if want.is_null() || want == current || !offered {
            if c["permission"] == true || !want.is_null() {
                applied["controls"][id] = current;
            }
            continue;
        }
        let res = if c["via"] == "mode" {
            conn.request("session/set_mode", json!({ "sessionId": sid, "modeId": want })).await
        } else {
            conn.request("session/set_config_option", json!({ "sessionId": sid, "configId": id, "value": want })).await
        };
        applied["controls"][id] = if res.is_ok() { want } else { current };
    }
    // Plan toggles are switched off; a plan mode never runs (Codex will not pursue /goal in one).
    for t in opts["planToggles"].as_array().cloned().unwrap_or_default() {
        if let Some(off) = crate::agents::plan_off_value(&t) {
            let id = t["id"].as_str().unwrap_or_default();
            if let Err(err) = conn
                .request("session/set_config_option", json!({ "sessionId": sid, "configId": id, "value": off }))
                .await
            {
                bail!("关闭 {} 的 {} 失败：{err}", m.connection_name, str_of_value(&t["name"], id));
            }
        }
    }
    if let Some(mode) = opts["controls"].as_array().and_then(|a| a.iter().find(|c| c["category"] == "mode")) {
        applied["mode"] = applied["controls"][mode["id"].as_str().unwrap_or_default()].clone();
    }
    applied["label"] = json!(applied_label(&opts, &applied["controls"]));
    Ok(applied)
}

pub(super) fn str_of_value<'a>(v: &'a Value, fallback: &'a str) -> &'a str {
    v.as_str().filter(|s| !s.is_empty()).unwrap_or(fallback)
}

/// "Agent · Plan 关闭": the permission controls a session actually runs with, by display name.
pub(super) fn applied_label(opts: &Value, values: &Value) -> String {
    let conn = json!({ "options": opts });
    crate::agents::session_label(values, &conn)
}

/// What limits this session, as shown with the attempt: only the agent's own permission mode.
pub(super) fn enforcement(applied: &Value) -> String {
    let mode = match applied["label"].as_str() {
        Some(l) if !l.is_empty() => format!("权限 {l}"),
        _ => "Agent 没有权限模式".into(),
    };
    format!("{mode}：直接在工作目录中执行，能做什么由 Agent 的权限模式决定")
}
