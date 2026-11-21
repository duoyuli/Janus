//! The MCP router (`janus-tools`): one MCP endpoint per run serving each session the MCP servers
//! pinned in its member's snapshot (the whole library), whatever their transport, in place of
//! handing each server to each session. A server starts (or is connected) on its
//! first call; a command runs in the run's working directory and a sandbox like an agent's. By
//! default the run's sessions share one backend of it (`McpProcess::PerRun`), stopped after `IDLE`
//! unused; a command server that keeps state runs once per session (`PerSession`) and ends with
//! the session. Listing never starts a server: it gives a running backend's list, or the list the
//! server last gave (kept on disk by revision), and only a server that never listed is started to
//! ask. Stopping, cancelling or failing the run stops them all, each command with the processes it
//! spawned (its process group). The transports themselves are in `mcp_client`.
// @project-doc docs/domains/20261116-resource_library.md#mcp_router

use super::mcp_client::Backend;
use super::*;
use crate::library::{Item, ItemKind, McpProcess, McpServer};
use std::sync::atomic::AtomicBool;

/// A shared server with no request for this long is stopped by the sweep; the next call starts it
/// again. It keeps no state worth holding on to (a stateful one runs per session, until its end).
const IDLE: Duration = Duration::from_secs(10 * 60);
/// How often the sweep looks for idle shared servers.
const SWEEP_EVERY: Duration = Duration::from_secs(60);
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

/// Who a running server serves: the whole run, or one session (`who`). Only a command server can
/// run per session; a URL server's connection is the run's.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Holder {
    Run,
    Session(String),
}

impl Holder {
    fn of(item: &Item, who: &str) -> Holder {
        match &item.body {
            crate::library::Body::Mcp { server: McpServer::Stdio { process: McpProcess::PerSession, .. } } => {
                Holder::Session(who.to_string())
            }
            _ => Holder::Run,
        }
    }
}

/// The run's servers, by who they serve and item id. Each slot is locked while its server starts,
/// so two requests at once start it once, while different servers start side by side.
#[derive(Default)]
pub(super) struct McpRouter {
    backends: StdMutex<HashMap<(Holder, String), Arc<Slot>>>,
    /// The idle sweep runs for this run.
    sweeping: AtomicBool,
    /// The run stopped: every server is stopped and none starts again.
    closed: AtomicBool,
    /// The tool calls in progress, by session (`who`) and the id the agent gave the request, so
    /// the agent's `notifications/cancelled` reaches the server doing the work.
    calls: StdMutex<HashMap<(String, String), Arc<Notify>>>,
}

/// A tool call in progress, until it ends; `cancel` with its session and request id stops it.
struct Calling<'a> {
    router: &'a McpRouter,
    key: (String, String),
    stop: Arc<Notify>,
}

impl Calling<'_> {
    async fn cancelled(&self) {
        self.stop.notified().await;
    }
}

impl Drop for Calling<'_> {
    fn drop(&mut self) {
        self.router.calls.lock().unwrap().remove(&self.key);
    }
}

/// One server's place in the router: empty until it starts.
type Slot = tokio::sync::Mutex<Option<Arc<Backend>>>;

impl McpRouter {
    /// A call of `who`'s request `id` starts; it can be cancelled until the returned value drops.
    fn calling(&self, who: &str, id: &Value) -> Calling<'_> {
        let key = (who.to_string(), id.to_string());
        let stop = Arc::new(Notify::new());
        self.calls.lock().unwrap().insert(key.clone(), stop.clone());
        Calling { router: self, key, stop }
    }

    /// The agent gave up on its request `id` (`notifications/cancelled`): the call stops waiting
    /// and tells the server. A request already answered, or never seen, is nothing to stop.
    fn cancel(&self, who: &str, id: &Value) {
        if let Some(stop) = self.calls.lock().unwrap().get(&(who.to_string(), id.to_string())) {
            // Kept for the call even when it has not started waiting yet.
            stop.notify_one();
        }
    }

    fn slot(&self, holder: &Holder, item: &str) -> Arc<Slot> {
        self.backends.lock().unwrap().entry((holder.clone(), item.to_string())).or_default().clone()
    }

    /// The process already serving `holder` for `item`, if one is running; never starts one.
    fn running(&self, holder: &Holder, item: &str) -> Option<Arc<Backend>> {
        let slot = self.backends.lock().unwrap().get(&(holder.clone(), item.to_string())).cloned()?;
        let held = slot.try_lock().ok()?;
        held.as_ref().filter(|b| b.usable()).cloned()
    }

    /// Stop the servers that `pick` selects, now; ones still starting are left to finish and be
    /// stopped by whoever finds them unusable.
    fn stop_where(&self, pick: impl Fn(&(Holder, String)) -> bool) {
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

    /// A session closed: the servers it ran on its own go with it; shared ones stay for the run.
    pub(super) fn stop_session(&self, who: &str) {
        self.stop_where(|(holder, _)| *holder == Holder::Session(who.to_string()));
    }

    /// Stop shared servers left unused for `IDLE`, and any that exited.
    fn sweep(&self) {
        let slots: Vec<Arc<Slot>> = self
            .backends
            .lock()
            .unwrap()
            .iter()
            .filter(|((holder, _), _)| *holder == Holder::Run)
            .map(|(_, s)| s.clone())
            .collect();
        for slot in slots {
            if let Ok(mut held) = slot.try_lock()
                && held.as_ref().is_some_and(|b| !b.usable() || b.idle_for() >= IDLE)
                && let Some(b) = held.take()
            {
                b.stop();
            }
        }
    }
}

/// A tool's name as the router lists it: `<server>__<tool>` when that fits `NAME_MAX` in the
/// characters MCP clients take and reads back as one pair; otherwise cut short and ended with a
/// hash of the full pair, so two tools never share a name and a name never changes between runs.
/// A plain name splits at its first `__`, so a server name holding `__` or ending in `_` always
/// takes the hash (server `a_` with tool `b` and server `a` with tool `_b` would both be `a___b`).
pub(super) fn routed_name(server: &str, tool: &str) -> String {
    let clean = |s: &str| {
        s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect::<String>()
    };
    let plain = format!("{server}{SEP}{tool}");
    let name = format!("{}{SEP}{}", clean(server), clean(tool));
    if name == plain && name.len() <= NAME_MAX && !server.contains(SEP) && !server.ends_with('_') {
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

    /// Prepare a server ahead of its runs: start a command once in its own sandbox (or connect to a
    /// URL), ask its tools, let it go. A command's package download lands in the sandbox, which
    /// runs reuse, so a run's start stays within the time agents give an MCP server; the tools it
    /// gave are what the router lists without starting it; and a server that can't start (a wrong
    /// command or address, a missing or wrong value) shows in the library now rather than in a run.
    /// Takes as long as it needs, in the background; the library shows how it went (`warm`).
    // @project-doc docs/domains/20261116-resource_library.md#mcp_router
    pub fn warm_mcp(self: &Arc<Self>, id: &str) {
        let Ok(item) = self.item(id) else { return };
        let crate::library::Body::Mcp { server } = item.body.clone() else { return };
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
                // Only a command runs here; a URL server needs no folder on this machine.
                if matches!(server, McpServer::Stdio { .. }) {
                    std::fs::create_dir_all(&cwd)?;
                }
                let b = Backend::start(&server, &app.mcp_values(&item.id), &home, &cwd, &log, WARM_WAIT).await?;
                let tools = b.tools(START_WAIT).await;
                b.stop();
                let tools = tools?;
                app.remember_tools(&item, &tools);
                Ok(tools.len())
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

    /// The servers the router serves the session `who`: the MCP servers in its member's snapshot
    /// (the lead's, or that of the member the task is assigned to), by their name in the snapshot,
    /// each as the library has it now. The snapshot decides which servers a run has (one added to
    /// the library later is not among them); the library decides how each runs (an edited command,
    /// address or value takes effect at its next start); one deleted from the library is gone from
    /// running runs too, neither listed nor started again.
    // @project-doc docs/domains/20261116-resource_library.md#delivery
    fn routed_servers(&self, run: &Run, who: &str) -> BTreeMap<String, Item> {
        let now: std::collections::HashMap<String, Item> = match self.items() {
            Ok(items) => items.into_iter().map(|i| (i.id.clone(), i)).collect(),
            Err(err) => {
                crate::config::log(format!("janus: the library could not be read for run {}: {err:#}", run.id));
                return BTreeMap::new();
            }
        };
        let member = if who == "lead" {
            run.team.iter().find(|m| m.lead)
        } else {
            let agent = run.st.lock().unwrap().tasks.iter().find(|t| t.def.id == who).map(|t| t.agent.clone());
            agent.and_then(|a| run.member(&a))
        };
        member
            .into_iter()
            .flat_map(|m| m.resources.iter().filter(|i| i.kind() == ItemKind::Mcp))
            .filter_map(|i| {
                let current = now.get(&i.id).filter(|c| c.kind() == ItemKind::Mcp)?;
                Some((i.name.clone(), Item { name: i.name.clone(), ..current.clone() }))
            })
            .collect()
    }

    /// The running backend of `item` for the session `who` (the run's shared one, or the
    /// session's own): the one already started, unless it exited or was stopped, in which case it
    /// starts now.
    async fn backend(self: &Arc<Self>, run: &Arc<Run>, who: &str, item: &Item) -> Result<Arc<Backend>> {
        if run.router.closed.load(Ordering::SeqCst) {
            bail!("这个任务已经停止");
        }
        let holder = Holder::of(item, who);
        let slot = run.router.slot(&holder, &item.id);
        let mut held = slot.lock().await;
        if let Some(b) = held.as_ref().filter(|b| b.usable()) {
            return Ok(b.clone());
        }
        if let Some(old) = held.take() {
            old.stop();
        }
        let crate::library::Body::Mcp { server } = &item.body else { bail!("{} 不是 MCP 服务器", item.name) };
        let tag = match &holder {
            Holder::Run => "run".to_string(),
            Holder::Session(who) => who.clone(),
        };
        let log = self.cfg.data_dir.join("logs").join(&run.id).join(format!("mcp-{tag}-{}.log", item.name));
        let home = self.server_home(&item.id);
        let b = Backend::start(server, &self.mcp_values(&item.id), &home, &run.cwd, &log, START_WAIT).await?;
        if run.router.closed.load(Ordering::SeqCst) {
            b.stop();
            bail!("这个任务已经停止");
        }
        *held = Some(b.clone());
        if holder == Holder::Run {
            Self::sweep_idle(run);
        }
        Ok(b)
    }

    /// Starts the run's idle sweep once; it ends with the run.
    fn sweep_idle(run: &Arc<Run>) {
        if run.router.sweeping.swap(true, Ordering::SeqCst) {
            return;
        }
        let run = Arc::downgrade(run);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(SWEEP_EVERY).await;
                let Some(run) = run.upgrade() else { break };
                if run.router.closed.load(Ordering::SeqCst) {
                    break;
                }
                run.router.sweep();
            }
        });
    }

    /// The list a server last gave, kept by revision: what listing shows without starting it.
    fn tools_cache(&self, item: &Item) -> PathBuf {
        self.tools_dir().join(format!("{}-{}.json", item.id, item.revision))
    }

    fn cached_tools(&self, item: &Item) -> Option<Vec<Value>> {
        serde_json::from_slice(&std::fs::read(self.tools_cache(item)).ok()?).ok()
    }

    /// A process's own list, written down for listing without starting it next time.
    fn remember_tools(&self, item: &Item, tools: &[Value]) {
        let path = self.tools_cache(item);
        let saved = match path.parent() {
            Some(dir) => std::fs::create_dir_all(dir),
            None => Ok(()),
        }
        .and_then(|_| std::fs::write(&path, serde_json::to_vec(tools).unwrap_or_default()));
        if let Err(err) = saved {
            crate::config::log(format!("janus: 记不下 {} 的工具列表：{err:#}", item.name));
        }
    }

    /// The server's tools from a running process (started if need be), written down.
    async fn live_tools(self: &Arc<Self>, run: &Arc<Run>, who: &str, item: &Item) -> Result<Vec<Value>> {
        let tools = self.backend(run, who, item).await?.tools(START_WAIT).await?;
        self.remember_tools(item, &tools);
        Ok(tools)
    }

    /// Each server's tools. Listing starts nothing it can avoid: a running process gives its own
    /// list, otherwise the list the server last gave; a server that never listed is started to
    /// ask, waiting at most `until` (it keeps starting after). With `fresh`, each server is asked
    /// by a running process, started if need be: a call looking for a tool the remembered list
    /// does not have.
    async fn routed_tools(
        self: &Arc<Self>,
        run: &Arc<Run>,
        who: &str,
        until: tokio::time::Instant,
        fresh: bool,
    ) -> Vec<(Item, Value)> {
        let mut out = vec![];
        let mut asking = vec![];
        for item in self.routed_servers(run, who).into_values() {
            let running = if fresh { None } else { run.router.running(&Holder::of(&item, who), &item.id) };
            let known = if fresh { None } else { self.cached_tools(&item) };
            let listed = match (running, known) {
                (Some(b), _) => Some(b.tools(START_WAIT).await),
                (None, Some(list)) => Some(Ok(list)),
                (None, None) => None,
            };
            match listed {
                Some(Ok(list)) => out.extend(list.into_iter().map(|t| (item.clone(), t))),
                Some(Err(err)) => crate::config::log(format!("janus: MCP 路由列不出 {} 的工具：{err:#}", item.name)),
                None => {
                    let (app, r, w, i) = (self.clone(), run.clone(), who.to_string(), item.clone());
                    asking.push((item, tokio::spawn(async move { app.live_tools(&r, &w, &i).await })));
                }
            }
        }
        for (item, job) in asking {
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
            if req["method"] == "notifications/cancelled" {
                run.router.cancel(who, &req["params"]["requestId"]);
            }
            return Ok(None);
        };
        let servers = self.routed_servers(&run, who);
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
                        "Janus 资源库中你能用的 MCP 服务器，经 Janus 路由：{}。工具名是「服务器{SEP}工具」（过长的名字截短并带哈希）；本机命令启动的服务器在这个任务的工作目录中运行。",
                        names.join("、")
                    ),
                })
            }
            "ping" => json!({}),
            "tools/list" => {
                let deadline = tokio::time::Instant::now() + LIST_WAIT;
                let tools: Vec<Value> = self
                    .routed_tools(&run, who, deadline, false)
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
                // From now on the agent can cancel it, even while the tool is still looked up.
                let calling = run.router.calling(who, &id);
                // The exact routed name: among the tools the session was shown, else among what the
                // servers give now (a tool added since the list was written down).
                let mut found = None;
                for (wait, fresh) in [(LIST_WAIT, false), (START_WAIT, true)] {
                    found = self
                        .routed_tools(&run, who, tokio::time::Instant::now() + wait, fresh)
                        .await
                        .into_iter()
                        .find_map(|(item, t)| {
                            let tool = t["name"].as_str()?;
                            (routed_name(&item.name, tool) == name).then(|| (item, tool.to_string()))
                        });
                    if found.is_some() {
                        break;
                    }
                }
                let outcome: Result<Value> = async {
                    let (item, tool) = found.as_ref().with_context(|| format!("没有这个工具：{name}"))?;
                    let args = req["params"]["arguments"].clone();
                    self.backend(&run, who, item)
                        .await?
                        .request_until(
                            "tools/call",
                            json!({ "name": tool, "arguments": args }),
                            CALL_WAIT,
                            calling.cancelled(),
                        )
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The sweep stops a shared server left unused for `IDLE`; a session's own server is the
    /// session's to end, however long it waits.
    #[tokio::test]
    async fn idle_shared_servers_stop_and_session_ones_wait_for_their_session() {
        let dir = std::env::temp_dir().join(format!("janus-sweep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/fake_mcp.py");
        let server = McpServer::Stdio {
            command: "python3".into(),
            args: vec![script.to_string_lossy().into()],
            env_keys: vec![],
            process: McpProcess::PerRun,
        };
        let (values, log) = (crate::library::McpValues::default(), dir.join("log"));
        let start = || Backend::start(&server, &values, &dir, &dir, &log, START_WAIT);
        let router = McpRouter::default();
        let (shared, own) = (start().await.unwrap(), start().await.unwrap());
        *router.slot(&Holder::Run, "s").try_lock().unwrap() = Some(shared.clone());
        *router.slot(&Holder::Session("T1".into()), "s").try_lock().unwrap() = Some(own.clone());
        let long_ago = std::time::Instant::now() - IDLE - Duration::from_secs(60);
        *shared.last_used.lock().unwrap() = long_ago;
        *own.last_used.lock().unwrap() = long_ago;
        router.sweep();
        assert!(!shared.usable(), "an idle shared server stops");
        assert!(own.usable(), "a session's own server stays until the session ends");
        router.stop_session("T1");
        assert!(!own.usable());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
