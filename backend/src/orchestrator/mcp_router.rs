//! The MCP router (`janus-tools`): one MCP endpoint per run serving the library's command
//! servers for everyone, in place of handing each server to each session. Each session (`who`)
//! gets processes of its own, as when an agent started its servers itself: started in the
//! background when the session opens, in the run's working directory and a sandbox like an
//! agent's. Listing waits a short while and lists the servers that are ready; a server's list is
//! kept while its process runs and asked again when it restarts or says its tools changed. A
//! session's servers live as long as the session, as they would under its agent; closing the
//! session, or stopping, cancelling or failing the run, stops them, each with the processes it
//! spawned (its process group).
// @project-doc docs/domains/20261116-resource_library.md#mcp_router

use super::*;
use crate::library::{Item, McpServer};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicBool, AtomicU64};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin};
use tokio::sync::oneshot;

/// How long a server may take to start (a first `npx -y` downloads its package) and to answer
/// `initialize` or `tools/list`.
const START_WAIT: Duration = Duration::from_secs(120);
/// How long preparing a server ahead of its runs may take: a first `npx -y` or `uvx` download of a
/// large package, once.
const WARM_WAIT: Duration = Duration::from_secs(10 * 60);
/// How long `tools/list` waits for the servers: agents give an MCP server about ten seconds to
/// start and list, and one slow server must not cost the session every routed tool.
const LIST_WAIT: Duration = Duration::from_secs(8);
/// How long one tool call may take.
const CALL_WAIT: Duration = Duration::from_secs(5 * 60);
/// What separates a server's name from its tool's in the names the router lists.
const SEP: &str = "__";
/// The longest name the router lists: models take tool names of at most 64 characters, and
/// agents put `mcp__janus-tools__` (18) in front.
const NAME_MAX: usize = 46;

/// The run's servers, one per session and item id. Each slot is locked while its server starts, so
/// two requests at once start it once, while different servers start side by side.
#[derive(Default)]
pub(super) struct McpRouter {
    backends: StdMutex<HashMap<(String, String), Arc<Slot>>>,
    /// The run stopped: every server is stopped and none starts again.
    closed: AtomicBool,
}

/// One server's place in the router: empty until it starts.
type Slot = tokio::sync::Mutex<Option<Arc<Backend>>>;

impl McpRouter {
    fn slot(&self, who: &str, item: &str) -> Arc<Slot> {
        self.backends.lock().unwrap().entry((who.to_string(), item.to_string())).or_default().clone()
    }

    /// Stop the servers that `pick` selects, now; ones still starting are left to finish and be
    /// stopped by whoever finds them unusable.
    fn stop_where(&self, pick: impl Fn(&(String, String)) -> bool) {
        let slots: Vec<Arc<Slot>> =
            self.backends.lock().unwrap().iter().filter(|(k, _)| pick(k)).map(|(_, s)| s.clone()).collect();
        for slot in slots {
            if let Ok(mut held) = slot.try_lock()
                && let Some(b) = held.take()
            {
                b.stop();
            }
        }
    }

    /// The run stopped: stop every server and start no more.
    pub(super) fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.stop_where(|_| true);
    }

    /// An item changed or went away: its running servers stop, and the next use starts it as it
    /// is now.
    pub(super) fn stop_item(&self, id: &str) {
        self.stop_where(|(_, item)| item == id);
    }

    /// A session closed: its servers go with it, as they would with its agent.
    pub(super) fn stop_session(&self, who: &str) {
        self.stop_where(|(session, _)| session == who);
    }
}

/// One server the router started: a command speaking MCP over stdio.
struct Backend {
    stdin: tokio::sync::Mutex<ChildStdin>,
    pending: Arc<StdMutex<HashMap<u64, oneshot::Sender<Value>>>>,
    next: AtomicU64,
    /// The tools this process listed; cleared when it says they changed.
    tools: Arc<StdMutex<Option<Vec<Value>>>>,
    /// False once its output has closed: the process is gone and the next use starts it again.
    alive: Arc<AtomicBool>,
    /// Its process group (`process_group(0)`): the server and whatever it spawned (`npx` runs the
    /// real server as a child).
    group: Option<u32>,
    stopped: AtomicBool,
    /// Killed with the backend (`kill_on_drop`) if the group's TERM did not end it.
    _child: Child,
}

impl Drop for Backend {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Backend {
    /// Start `server` in `cwd` and initialize it, in a sandbox of its own (`sandbox_at(home)`),
    /// never in Janus's environment: it sees the sandbox variables and the values saved on it,
    /// which can't override them. Its stderr goes to `log`; `roots/list` from it is answered with
    /// `cwd`.
    // @project-doc docs/operations/20261031-security.md#library
    async fn start(
        server: &McpServer,
        values: &crate::library::McpValues,
        home: &Path,
        cwd: &Path,
        log: &Path,
        wait: Duration,
    ) -> Result<Arc<Backend>> {
        let McpServer::Stdio { command, args, .. } = server else {
            bail!("MCP 路由只运行命令启动的服务器");
        };
        if let Some(dir) = log.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let err = std::fs::OpenOptions::new().create(true).append(true).open(log)?;
        let own = values.env.iter().filter(|(k, _)| !crate::agents::is_sandbox_var(k));
        let mut cmd = tokio::process::Command::new(command);
        #[cfg(unix)]
        cmd.process_group(0);
        let mut child = cmd
            .args(args)
            .env_clear()
            .envs(crate::agents::sandbox_at(home)?)
            .envs(own)
            .current_dir(cwd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::from(err))
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("启动 {command} 失败"))?;
        let stdin = child.stdin.take().context("no stdin")?;
        let stdout = child.stdout.take().context("no stdout")?;
        let pending: Arc<StdMutex<HashMap<u64, oneshot::Sender<Value>>>> = Arc::default();
        let tools: Arc<StdMutex<Option<Vec<Value>>>> = Arc::default();
        let alive = Arc::new(AtomicBool::new(true));
        let backend = Backend {
            stdin: tokio::sync::Mutex::new(stdin),
            pending: pending.clone(),
            next: AtomicU64::new(1),
            tools: tools.clone(),
            alive: alive.clone(),
            group: child.id(),
            stopped: AtomicBool::new(false),
            _child: child,
        };
        // Replies go to whoever waits on their id; the server's own requests are answered here.
        let (requests_tx, mut requests) = mpsc::unbounded_channel::<Value>();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
                if msg.get("method").is_some() {
                    if msg.get("id").is_some() {
                        let _ = requests_tx.send(msg);
                    } else if msg["method"] == "notifications/tools/list_changed" {
                        *tools.lock().unwrap() = None;
                    }
                    continue;
                }
                if let Some(id) = msg["id"].as_u64()
                    && let Some(tx) = pending.lock().unwrap().remove(&id)
                {
                    let _ = tx.send(msg);
                }
            }
            // The server is gone: whoever still waits gets an error instead of hanging.
            alive.store(false, Ordering::SeqCst);
            pending.lock().unwrap().clear();
        });
        let root = format!("file://{}", cwd.display());
        let backend = Arc::new(backend);
        let answering = Arc::downgrade(&backend);
        tokio::spawn(async move {
            while let Some(req) = requests.recv().await {
                let Some(b) = answering.upgrade() else { break };
                let reply = match req["method"].as_str() {
                    Some("roots/list") => {
                        json!({ "jsonrpc": "2.0", "id": req["id"], "result": { "roots": [{ "uri": root, "name": "workspace" }] } })
                    }
                    Some("ping") => json!({ "jsonrpc": "2.0", "id": req["id"], "result": {} }),
                    _ => {
                        json!({ "jsonrpc": "2.0", "id": req["id"], "error": { "code": -32601, "message": "Janus 不支持这个请求" } })
                    }
                };
                let _ = b.send(&reply).await;
            }
        });
        let init = json!({
            "protocolVersion": "2025-06-18",
            "capabilities": { "roots": { "listChanged": false } },
            "clientInfo": { "name": "janus", "version": crate::VERSION }
        });
        backend.request("initialize", init, wait).await?;
        backend.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).await?;
        Ok(backend)
    }

    /// Its tools: the list this process gave, or asked now.
    async fn tools(&self) -> Result<Vec<Value>> {
        if let Some(t) = self.tools.lock().unwrap().clone() {
            return Ok(t);
        }
        let t =
            self.request("tools/list", json!({}), START_WAIT).await?["tools"].as_array().cloned().unwrap_or_default();
        *self.tools.lock().unwrap() = Some(t.clone());
        Ok(t)
    }

    /// Still running and not stopped.
    fn usable(&self) -> bool {
        !self.stopped.load(Ordering::SeqCst) && self.alive.load(Ordering::SeqCst)
    }

    /// End its process group: TERM now, KILL a few seconds later for whatever ignored it. A call
    /// still waiting on it gets an error.
    fn stop(&self) {
        if self.stopped.swap(true, Ordering::SeqCst) {
            return;
        }
        self.pending.lock().unwrap().clear();
        #[cfg(unix)]
        if let Some(pid) = self.group {
            let group = format!("-{pid}");
            let _ = std::process::Command::new("kill").args(["-TERM", &group]).status();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(3));
                let _ = std::process::Command::new("kill").args(["-KILL", &group]).status();
            });
        }
    }

    async fn send(&self, msg: &Value) -> Result<()> {
        let mut line = serde_json::to_vec(msg)?;
        line.push(b'\n');
        let mut stdin = self.stdin.lock().await;
        stdin.write_all(&line).await?;
        stdin.flush().await?;
        Ok(())
    }

    /// One request; its result, or the server's error as an error.
    async fn request(&self, method: &str, params: Value, wait: Duration) -> Result<Value> {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })).await?;
        let reply = match tokio::time::timeout(wait, rx).await {
            Ok(Ok(reply)) => reply,
            Ok(Err(_)) => bail!("服务器已经退出"),
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                bail!("{} 秒内没有回应", wait.as_secs())
            }
        };
        if let Some(err) = reply.get("error") {
            bail!("{}", err["message"].as_str().unwrap_or("服务器返回了错误"));
        }
        Ok(reply["result"].clone())
    }
}

/// A tool's name as the router lists it: `<server>__<tool>` when that fits `NAME_MAX` in the
/// characters MCP clients take; otherwise cut short and ended with a hash of the full pair, so two
/// tools never share a name and a name never changes between runs.
pub(super) fn routed_name(server: &str, tool: &str) -> String {
    let clean = |s: &str| {
        s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect::<String>()
    };
    let plain = format!("{server}{SEP}{tool}");
    let name = format!("{}{SEP}{}", clean(server), clean(tool));
    if name == plain && name.len() <= NAME_MAX {
        return name;
    }
    let hash: String =
        Sha256::digest(format!("{server}\0{tool}").as_bytes()).iter().take(4).map(|b| format!("{b:02x}")).collect();
    format!("{}_{hash}", &name[..name.len().min(NAME_MAX - hash.len() - 1)])
}

impl App {
    /// The router's entry for a session of `who`, beside the board's: same token rule.
    pub(super) fn tools_server(&self, run: &Run, who: &str) -> Value {
        let token = session_token(&run.st.lock().unwrap().mcp_token, who);
        json!({
            "type": "http",
            "name": crate::library::MCP_ROUTER,
            "url": format!("http://{}/mcp/{}/{}/{}/tools", self.cfg.addr, run.id, token, who),
            "headers": []
        })
    }

    /// Prepare a command server ahead of its runs: start it once in its own sandbox, ask its tools,
    /// stop it. Its package download lands in the sandbox, which runs reuse, so a run's start stays
    /// within the time agents give an MCP server; and a server that can't start (a wrong command, a
    /// missing value) shows in the library now rather than in a run. Takes as long as it needs, in
    /// the background; the library shows how it went (`warm`).
    // @project-doc docs/domains/20261116-resource_library.md#mcp_router
    pub fn warm_mcp(self: &Arc<Self>, id: &str) {
        let Ok(item) = self.item(id) else { return };
        let crate::library::Body::Mcp { server: server @ McpServer::Stdio { .. } } = item.body.clone() else { return };
        let revision = item.revision.clone();
        self.library
            .warm
            .lock()
            .unwrap()
            .insert(item.id.clone(), crate::library::Warm::Preparing { revision: revision.clone() });
        let app = self.clone();
        tokio::spawn(async move {
            let home = app.server_home(&item.id);
            let cwd = home.join("warm");
            let log = app.cfg.data_dir.join("logs").join("library").join(format!("mcp-{}.log", item.name));
            let got: Result<usize> = async {
                std::fs::create_dir_all(&cwd)?;
                let b = Backend::start(&server, &app.mcp_values(&item.id), &home, &cwd, &log, WARM_WAIT).await?;
                let tools = b.tools().await;
                b.stop();
                Ok(tools?.len())
            }
            .await;
            let done = match got {
                Ok(tools) => crate::library::Warm::Ready { revision: revision.clone(), tools },
                Err(err) => crate::library::Warm::Failed { revision: revision.clone(), error: format!("{err:#}") },
            };
            // A later save prepares the newer revision; this result only stands for its own.
            let mut warm = app.library.warm.lock().unwrap();
            if warm.get(&item.id).is_some_and(|w| w.revision() == revision) {
                warm.insert(item.id.clone(), done);
            }
        });
    }

    /// A library item changed or was deleted: every run stops its servers, so the next use starts
    /// it as it is now (or not at all).
    pub(crate) fn stop_item_servers(&self, id: &str) {
        for run in self.runs.lock().unwrap().values() {
            run.router.stop_item(id);
        }
    }

    /// The servers the router serves for this run: the command servers for everyone in its
    /// members' snapshots, by name.
    fn routed_servers(run: &Run) -> BTreeMap<String, Item> {
        run.team
            .iter()
            .flat_map(|m| m.resources.iter().filter(|i| crate::library::routed_server(i)))
            .map(|i| (i.name.clone(), i.clone()))
            .collect()
    }

    /// The running backend of `item` for the session `who`: the one already started, unless it
    /// exited, in which case it starts again (and lists its tools afresh).
    async fn backend(self: &Arc<Self>, run: &Arc<Run>, who: &str, item: &Item) -> Result<Arc<Backend>> {
        if run.router.closed.load(Ordering::SeqCst) {
            bail!("这个任务已经停止");
        }
        let slot = run.router.slot(who, &item.id);
        let mut held = slot.lock().await;
        if let Some(b) = held.as_ref().filter(|b| b.usable()) {
            return Ok(b.clone());
        }
        if let Some(old) = held.take() {
            old.stop();
        }
        let crate::library::Body::Mcp { server } = &item.body else { bail!("{} 不是 MCP 服务器", item.name) };
        let log = self.cfg.data_dir.join("logs").join(&run.id).join(format!("mcp-{who}-{}.log", item.name));
        let home = self.server_home(&item.id);
        let b = Backend::start(server, &self.mcp_values(&item.id), &home, &run.cwd, &log, START_WAIT).await?;
        if run.router.closed.load(Ordering::SeqCst) {
            b.stop();
            bail!("这个任务已经停止");
        }
        *held = Some(b.clone());
        Ok(b)
    }

    async fn server_tools(self: &Arc<Self>, run: &Arc<Run>, who: &str, item: &Item) -> Result<Vec<Value>> {
        self.backend(run, who, item).await?.tools().await
    }

    /// A session that got the router (`via: mcpRouter`) has its servers started now, in the
    /// background, so they are ready by the time its agent lists them.
    pub(super) fn prewarm(self: &Arc<Self>, run: &Arc<Run>, who: &str, opening: &Opening) {
        if !opening.resources.iter().any(|d| d.via == crate::library::Via::McpRouter) {
            return;
        }
        for item in Self::routed_servers(run).into_values() {
            let (app, run, who) = (self.clone(), run.clone(), who.to_string());
            tokio::spawn(async move {
                if let Err(err) = app.server_tools(&run, &who, &item).await {
                    crate::config::log(format!("janus: MCP 路由启动 {} 失败：{err:#}", item.name));
                }
            });
        }
    }

    /// Each server's tools under their routed names, waiting at most `until`: a server still
    /// starting is left out and keeps starting.
    async fn routed_tools(
        self: &Arc<Self>,
        run: &Arc<Run>,
        who: &str,
        until: tokio::time::Instant,
    ) -> Vec<(Item, Value)> {
        let mut listing = vec![];
        for item in Self::routed_servers(run).into_values() {
            let (app, r, w, i) = (self.clone(), run.clone(), who.to_string(), item.clone());
            listing.push((item, tokio::spawn(async move { app.server_tools(&r, &w, &i).await })));
        }
        let mut out = vec![];
        for (item, job) in listing {
            match tokio::time::timeout_at(until, job).await {
                Ok(Ok(Ok(list))) => out.extend(list.into_iter().map(|t| (item.clone(), t))),
                Ok(Ok(Err(err))) => crate::config::log(format!("janus: MCP 路由列不出 {} 的工具：{err:#}", item.name)),
                Ok(Err(_)) => {}
                Err(_) => crate::config::log(format!("janus: MCP 路由的 {} 还在启动，这次先不列出", item.name)),
            }
        }
        out
    }

    /// One MCP request to the router from a session of this run (`who` as for the board).
    pub async fn mcp_tools(
        self: &Arc<Self>,
        run_id: &str,
        token: &str,
        who: &str,
        req: Value,
    ) -> Result<Option<Value>> {
        let run = self.run(run_id).context("这个任务不在运行")?;
        let run_token = run.st.lock().unwrap().mcp_token.clone();
        if run_token.is_empty() || token != session_token(&run_token, who) {
            bail!("token 不对");
        }
        let Some(id) = req.get("id").cloned() else {
            return Ok(None);
        };
        let servers = Self::routed_servers(&run);
        let result = match req["method"].as_str().unwrap_or("") {
            "initialize" => {
                let names: Vec<String> = servers
                    .values()
                    .map(|i| {
                        if i.description.is_empty() {
                            i.name.clone()
                        } else {
                            format!("{}（{}）", i.name, short(&i.description, 80))
                        }
                    })
                    .collect();
                json!({
                    "protocolVersion": req["params"]["protocolVersion"].as_str().unwrap_or("2025-06-18"),
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": { "name": crate::library::MCP_ROUTER, "version": crate::VERSION },
                    "instructions": format!(
                        "Janus 资源库中给所有数字员工的 MCP 服务器，经 Janus 路由：{}。工具名是「服务器{SEP}工具」（过长的名字截短并带哈希）；服务器在这个会话里单独运行，工作目录是这个任务的工作目录。",
                        names.join("、")
                    ),
                })
            }
            "ping" => json!({}),
            "tools/list" => {
                let deadline = tokio::time::Instant::now() + LIST_WAIT;
                let tools: Vec<Value> = self
                    .routed_tools(&run, who, deadline)
                    .await
                    .into_iter()
                    .map(|(item, mut t)| {
                        t["name"] = json!(routed_name(&item.name, t["name"].as_str().unwrap_or_default()));
                        t
                    })
                    .collect();
                json!({ "tools": tools })
            }
            "tools/call" => {
                let name = req["params"]["name"].as_str().unwrap_or_default();
                let started = now_ms();
                // The exact routed name among the servers' current tools: those ready first, then
                // waiting for any still starting.
                let mut found = None;
                for wait in [LIST_WAIT, START_WAIT] {
                    found =
                        self.routed_tools(&run, who, tokio::time::Instant::now() + wait).await.into_iter().find_map(
                            |(item, t)| {
                                let tool = t["name"].as_str()?;
                                (routed_name(&item.name, tool) == name).then(|| (item, tool.to_string()))
                            },
                        );
                    if found.is_some() {
                        break;
                    }
                }
                let outcome: Result<Value> = async {
                    let (item, tool) = found.as_ref().with_context(|| format!("没有这个工具：{name}"))?;
                    let args = req["params"]["arguments"].clone();
                    self.backend(&run, who, item)
                        .await?
                        .request("tools/call", json!({ "name": tool, "arguments": args }), CALL_WAIT)
                        .await
                }
                .await;
                // Who called which server's tool, how long it took and how it ended; never the
                // arguments, which may carry what the person typed or a credential.
                let mut e = ev("mcp.routed", who, format!("经 MCP 路由调用 {name}")).data(json!({
                    "server": found.as_ref().map(|(i, _)| i.name.clone()),
                    "tool": name,
                    "ms": now_ms() - started,
                    "error": outcome.as_ref().err().map(|e| format!("{e:#}")),
                }));
                if who != "lead" {
                    e = e.task(who);
                }
                self.emit(&run, e);
                // After handing out subtasks the lead only looks: a call it makes is put in front
                // of it on the board, once per tool, and goes on the delivery as a risk.
                // @project-doc docs/domains/20261031-goal_mode.md#lead_role
                let after_plan = who == "lead" && found.is_some() && {
                    let mut st = run.st.lock().unwrap();
                    !st.tasks.is_empty() && st.traces.record_lead_tool_after_plan(name)
                };
                if after_plan {
                    self.emit(
                        &run,
                        ev("lead.routed", "janus", format!("负责人派出子任务后调用了 {name}"))
                            .to("lead")
                            .data(json!({ "tool": name, "server": found.as_ref().map(|(i, _)| i.name.clone()) })),
                    );
                }
                match outcome {
                    Ok(result) => result,
                    Err(err) => json!({ "content": [{ "type": "text", "text": format!("{err:#}") }], "isError": true }),
                }
            }
            other => {
                return Ok(Some(
                    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": format!("不支持 {other}") } }),
                ));
            }
        };
        Ok(Some(json!({ "jsonrpc": "2.0", "id": id, "result": result })))
    }
}
