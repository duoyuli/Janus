//! The MCP router (`janus-tools`): one MCP endpoint per run serving each session the MCP servers
//! pinned in its member's snapshot (the whole library), whatever their transport, in place of
//! handing each server to each session. After magpie's MCP router, the agent is given three tools
//! and never the servers' own: `find_tools` finds them (`find`), `call_tool` runs one by name, and
//! `run_code` runs a script that calls them (`code`); `initialize`'s instructions name each server,
//! what it is for and its tools (`say`). A server starts (or is connected) on its first call; a
//! command runs in the run's working directory and a sandbox like an agent's. The run's sessions
//! share one backend of each, stopped after `IDLE` unused. What a server said of itself and the
//! tools it listed are kept on disk by revision (a card), so describing and listing start nothing
//! that ever answered. Stopping, cancelling or failing the run stops them all, each command with
//! the processes it spawned (its process group). The transports themselves are in `mcp_client`.
// @project-doc docs/domains/20261116-resource_library.md#mcp_router

use super::mcp_client::{Backend, Said};
use super::*;
use crate::lang::{Lang, tr};
use crate::library::{Item, ItemKind, McpServer};
use std::sync::atomic::AtomicBool;
use tokio_util::sync::CancellationToken;

mod code;
mod find;
mod say;

/// A server with no request for this long is stopped by the sweep; the next call starts it again.
const IDLE: Duration = Duration::from_secs(10 * 60);
/// How often the sweep looks for idle servers.
const SWEEP_EVERY: Duration = Duration::from_secs(60);
/// How long a server may take to start (a first `npx -y` downloads its package) and to answer
/// `initialize` or `tools/list`.
const START_WAIT: Duration = Duration::from_secs(120);
/// How long preparing a server ahead of its runs may take: a first `npx -y` or `uvx` download of a
/// large package, once.
const WARM_WAIT: Duration = Duration::from_secs(10 * 60);
/// How long listing waits for the servers: agents give an MCP server about ten seconds to start
/// and list, and one slow server must not cost the session every routed tool.
const LIST_WAIT: Duration = Duration::from_secs(8);
/// How long `initialize` waits for servers that never described themselves, to name their tools.
const DESCRIBE_WAIT: Duration = Duration::from_secs(5);
/// How long one tool call may take: under the 60 seconds Claude Code and Codex give an MCP call,
/// so the agent gets the router's answer and not its own time-out.
const CALL_WAIT: Duration = Duration::from_secs(50);
/// What separates a server's name from its tool's in the names the router gives.
const SEP: &str = "__";
/// The longest name the router gives: models take tool names of at most 64 characters, and
/// agents put `mcp__janus-tools__` (18) in front of a name they are given directly.
const NAME_MAX: usize = 46;

/// One tool of a server behind the router: its server as the library has it now, its own name,
/// the name the router gives it (`routed_name`) and its definition as the server listed it.
#[derive(Clone)]
pub(super) struct Routed {
    pub item: Item,
    pub tool: String,
    pub name: String,
    pub spec: Value,
}

/// What the router knows of one server without starting it: what it said of itself and the tools
/// it listed, kept on disk for the server's revision.
#[derive(Clone, Default, Serialize, Deserialize)]
struct Card {
    #[serde(flatten)]
    said: Said,
    tools: Vec<Value>,
}

/// The run's servers, by item id. Each slot is locked while its server starts, so two requests at
/// once start it once, while different servers start side by side.
#[derive(Default)]
pub(super) struct McpRouter {
    backends: StdMutex<HashMap<String, Arc<Slot>>>,
    /// The idle sweep runs for this run.
    sweeping: AtomicBool,
    /// The run stopped: every server is stopped and none starts again.
    closed: AtomicBool,
    /// The tool calls in progress, by session and the id the agent gave the request, so
    /// the agent's `notifications/cancelled` reaches the server doing the work: every call a
    /// `run_code` script makes goes with its request.
    calls: StdMutex<HashMap<(SessionKey, String), CancellationToken>>,
}

/// A request in progress, until it ends; `cancel` with its session and request id stops it.
struct Calling<'a> {
    router: &'a McpRouter,
    key: (SessionKey, String),
    stop: CancellationToken,
}

impl Drop for Calling<'_> {
    fn drop(&mut self) {
        self.router.calls.lock().unwrap().remove(&self.key);
    }
}

/// One server's place in the router: empty until it starts.
type Slot = tokio::sync::Mutex<Option<Arc<Backend>>>;

impl McpRouter {
    /// A call of `seat`'s request `id` starts; it can be cancelled until the returned value drops.
    fn calling(&self, seat: &SessionKey, id: &Value) -> Calling<'_> {
        let key = (seat.clone(), id.to_string());
        let stop = CancellationToken::new();
        self.calls.lock().unwrap().insert(key.clone(), stop.clone());
        Calling { router: self, key, stop }
    }

    /// The agent gave up on its request `id` (`notifications/cancelled`): the call stops waiting
    /// and tells the server. A request already answered, or never seen, is nothing to stop.
    fn cancel(&self, seat: &SessionKey, id: &Value) {
        if let Some(stop) = self.calls.lock().unwrap().get(&(seat.clone(), id.to_string())) {
            stop.cancel();
        }
    }

    fn slot(&self, item: &str) -> Arc<Slot> {
        self.backends.lock().unwrap().entry(item.to_string()).or_default().clone()
    }

    /// The process already serving `item`, if one is running; never starts one.
    fn running(&self, item: &str) -> Option<Arc<Backend>> {
        let slot = self.backends.lock().unwrap().get(item).cloned()?;
        let held = slot.try_lock().ok()?;
        held.as_ref().filter(|b| b.usable()).cloned()
    }

    /// Stop the servers that `pick` selects, now; ones still starting are left to finish and be
    /// stopped by whoever finds them unusable.
    fn stop_where(&self, pick: impl Fn(&str) -> bool) {
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

    /// An item changed or went away: its running server stops, and the next use starts it as it
    /// is now.
    pub(super) fn stop_item(&self, id: &str) {
        self.stop_where(|item| item == id);
    }

    /// Stop servers left unused for `IDLE`, and any that exited.
    fn sweep(&self) {
        let slots: Vec<Arc<Slot>> = self.backends.lock().unwrap().values().cloned().collect();
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

/// The run has stopped: no server starts for it any more.
fn stopped(lang: Lang) -> anyhow::Error {
    anyhow!(tr!(lang, "这个任务已经停止", "This run has stopped"))
}

/// Whether the router can call the tool: one that only runs as a task (`execution.taskSupport`
/// `required`, MCP 2025-11-25) needs task-augmented calls, which the router does not make.
fn callable(tool: &Value) -> bool {
    tool["execution"]["taskSupport"] != "required"
}

/// A result of the router's own, as text; `failed` marks it an error the model can act on.
fn said(text: String, failed: bool) -> Value {
    tool_text(&text, failed)
}

/// The member whose snapshot serves the session `seat`: the lead's, or that of the member the task
/// is assigned to.
fn session_member<'a>(run: &'a Run, seat: &SessionKey) -> Option<&'a Member> {
    match seat {
        SessionKey::Lead => run.team.iter().find(|m| m.lead),
        SessionKey::Task(task) => {
            let agent = run.st.lock().unwrap().tasks.iter().find(|t| t.def.id == *task).map(|t| t.agent.clone());
            agent.and_then(|a| run.member(&a))
        }
    }
}

impl App {
    /// The router's entry for a session in `seat`, beside the board's: same token rule.
    pub(super) fn tools_server(&self, run: &Run, seat: &SessionKey) -> Value {
        self.session_server(run, seat, crate::library::MCP_ROUTER, "/tools")
    }

    /// Prepare a server ahead of its runs: start a command once in its own sandbox (or connect to a
    /// URL), ask its tools, let it go. A command's package download lands in the sandbox, which
    /// runs reuse, so a run's start stays within the time agents give an MCP server; what it said
    /// of itself and the tools it gave are what the router describes and finds without starting
    /// it; and a server that can't start (a wrong command or address, a missing or wrong value)
    /// shows in the library now rather than in a run. Takes as long as it needs, in the background;
    /// the library shows how it went (`warm`).
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
            let sandbox = app.cfg.layout.server_sandbox(&item.id);
            let cwd = sandbox.tmp.join("warm");
            if let Err(e) = app.link_host(&sandbox) {
                crate::config::log(format!("没能读出共享的本机环境：{e:#}"));
            }
            let log = app.cfg.layout.library_log(&item.id);
            let got: Result<usize> = async {
                // Only a command runs here; a URL server needs no folder on this machine.
                if matches!(server, McpServer::Stdio { .. }) {
                    std::fs::create_dir_all(&cwd)?;
                }
                let b = Backend::start(
                    &server,
                    &app.mcp_values(&item.id),
                    &sandbox,
                    &cwd,
                    || crate::acp::open_log(&log),
                    WARM_WAIT,
                )
                .await?;
                let tools = b.tools(START_WAIT).await;
                b.stop();
                let card = Card { said: b.said.lock().unwrap().clone(), tools: tools? };
                app.remember_card(&item, &card);
                Ok(card.tools.iter().filter(|t| callable(t)).count())
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

    /// A library item changed or was deleted: every run stops its server, so the next use starts
    /// it as it is now (or not at all).
    pub(crate) fn stop_item_servers(&self, id: &str) {
        for run in self.runs.lock().unwrap().values() {
            run.router.stop_item(id);
        }
    }

    /// The servers the router serves the session `seat`: the MCP servers in its member's snapshot
    /// (the lead's, or that of the member the task is assigned to), by their name in the snapshot,
    /// each as the library has it now. The snapshot decides which servers a run has (one added to
    /// the library later is not among them); the library decides how each runs (an edited command,
    /// address or value takes effect at its next start); one deleted from the library is gone from
    /// running runs too, neither listed nor started again.
    // @project-doc docs/domains/20261116-resource_library.md#delivery
    fn routed_servers(&self, run: &Run, seat: &SessionKey) -> BTreeMap<String, Item> {
        let now: std::collections::HashMap<String, Item> = match self.items() {
            Ok(items) => items.into_iter().map(|i| (i.id.clone(), i)).collect(),
            Err(err) => {
                crate::config::log(format!("janus: the library could not be read for run {}: {err:#}", run.id));
                return BTreeMap::new();
            }
        };
        session_member(run, seat)
            .into_iter()
            .flat_map(|m| m.resources.iter().filter(|i| i.kind() == ItemKind::Mcp))
            .filter_map(|i| {
                let current = now.get(&i.id).filter(|c| c.kind() == ItemKind::Mcp)?;
                Some((i.name.clone(), Item { name: i.name.clone(), ..current.clone() }))
            })
            .collect()
    }

    /// The running backend of `item` for the run: the one already started, unless it exited or was
    /// stopped, in which case it starts now.
    async fn backend(self: &Arc<Self>, run: &Arc<Run>, item: &Item) -> Result<Arc<Backend>> {
        if run.router.closed.load(Ordering::SeqCst) {
            return Err(stopped(run.lang));
        }
        let slot = run.router.slot(&item.id);
        let mut held = slot.lock().await;
        if let Some(b) = held.as_ref().filter(|b| b.usable()) {
            return Ok(b.clone());
        }
        if let Some(old) = held.take() {
            old.stop();
        }
        let crate::library::Body::Mcp { server } = &item.body else {
            bail!(tr!(run.lang, "{} 不是 MCP 服务器", "{} is not an MCP server", item.name))
        };
        let log = self.cfg.layout.run(run.kind, &run.id).log(&format!("mcp-{}.log", item.name));
        let sandbox = self.cfg.layout.server_sandbox(&item.id);
        self.link_host(&sandbox)?;
        // Its log goes in the run's folders, so not once the run is closed for deleting (`Run.files`).
        let open = || {
            let _files = run.files.read().unwrap_or_else(|p| p.into_inner());
            if run.router.closed.load(Ordering::SeqCst) {
                return Err(stopped(run.lang));
            }
            crate::acp::open_log(&log)
        };
        let b = Backend::start(server, &self.mcp_values(&item.id), &sandbox, &run.cwd, open, START_WAIT).await?;
        if run.router.closed.load(Ordering::SeqCst) {
            b.stop();
            return Err(stopped(run.lang));
        }
        *held = Some(b.clone());
        Self::sweep_idle(run);
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

    /// The card a server last gave, kept for its revision: what describing and listing show without
    /// starting it.
    fn card_path(&self, item: &Item) -> PathBuf {
        self.cfg.layout.mcp_tools(&item.id).join(format!("{}.json", item.revision))
    }

    fn card(&self, item: &Item) -> Option<Card> {
        serde_json::from_slice(&std::fs::read(self.card_path(item)).ok()?).ok()
    }

    /// A process's own card, written down for describing and listing without starting it next
    /// time. Only the current revision's card is kept: an older one is never read again.
    fn remember_card(&self, item: &Item, card: &Card) {
        let path = self.card_path(item);
        let dir = self.cfg.layout.mcp_tools(&item.id);
        // Written whole: a run reading it meanwhile never sees half a card.
        if let Err(err) = crate::util::write_whole(&path, &serde_json::to_vec(card).unwrap_or_default()) {
            crate::config::log(format!("janus: 记不下 {} 的工具列表：{err:#}", item.name));
        }
        for old in std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path() != path && !e.file_name().to_string_lossy().starts_with('.'))
        {
            let _ = std::fs::remove_file(old.path());
        }
    }

    /// A running process's card (started if need be), written down.
    async fn live_card(self: &Arc<Self>, run: &Arc<Run>, item: &Item) -> Result<Card> {
        let b = self.backend(run, item).await?;
        let card = Card { tools: b.tools(START_WAIT).await?, said: b.said.lock().unwrap().clone() };
        self.remember_card(item, &card);
        Ok(card)
    }

    /// Each server's card. Starts nothing it can avoid: a running process gives its own, otherwise
    /// the card the server last gave; a server that never gave one is started to ask, waiting at
    /// most `until` (it keeps starting after). With `fresh`, each server is asked by a running
    /// process, started if need be: a call looking for a tool the remembered card does not have.
    async fn cards(
        self: &Arc<Self>,
        run: &Arc<Run>,
        seat: &SessionKey,
        until: tokio::time::Instant,
        fresh: bool,
    ) -> Vec<(Item, Card)> {
        let mut out = vec![];
        let mut asking = vec![];
        for item in self.routed_servers(run, seat).into_values() {
            let running = if fresh { None } else { run.router.running(&item.id) };
            let known = if fresh { None } else { self.card(&item) };
            let listed = match (running, known) {
                (Some(b), _) => {
                    Some(b.tools(START_WAIT).await.map(|tools| Card { tools, said: b.said.lock().unwrap().clone() }))
                }
                (None, Some(card)) => Some(Ok(card)),
                (None, None) => None,
            };
            match listed {
                Some(Ok(card)) => out.push((item, card)),
                Some(Err(err)) => crate::config::log(format!("janus: MCP 路由列不出 {} 的工具：{err:#}", item.name)),
                None => {
                    let (app, r, i) = (self.clone(), run.clone(), item.clone());
                    asking.push((item, tokio::spawn(async move { app.live_card(&r, &i).await })));
                }
            }
        }
        for (item, job) in asking {
            match tokio::time::timeout_at(until, job).await {
                Ok(Ok(Ok(card))) => out.push((item, card)),
                Ok(Ok(Err(err))) => crate::config::log(format!("janus: MCP 路由列不出 {} 的工具：{err:#}", item.name)),
                Ok(Err(_)) => {}
                Err(_) => crate::config::log(format!("janus: MCP 路由的 {} 还在启动，这次先不列出", item.name)),
            }
        }
        out
    }

    /// Every tool behind the router for the session `seat`, as `cards` finds them.
    async fn routed_tools(
        self: &Arc<Self>,
        run: &Arc<Run>,
        seat: &SessionKey,
        until: tokio::time::Instant,
        fresh: bool,
    ) -> Vec<Routed> {
        self.cards(run, seat, until, fresh)
            .await
            .into_iter()
            .flat_map(|(item, card)| {
                card.tools.into_iter().filter(callable).map(move |spec| {
                    let tool = spec["name"].as_str().unwrap_or_default().to_string();
                    Routed { name: routed_name(&item.name, &tool), tool, item: item.clone(), spec }
                })
            })
            .collect()
    }

    /// The servers behind the router as its words name them: what each is for (the library's
    /// description, else what the server says of itself), its title and its tools.
    async fn described(self: &Arc<Self>, run: &Arc<Run>, seat: &SessionKey, wait: Duration) -> Vec<say::Server> {
        self.cards(run, seat, tokio::time::Instant::now() + wait, false)
            .await
            .into_iter()
            .map(|(item, card)| say::Server {
                about: if item.description.trim().is_empty() { card.said.says } else { item.description.clone() },
                title: card.said.title,
                tools: card
                    .tools
                    .iter()
                    .filter(|t| callable(t))
                    .map(|t| {
                        let s = |k: &str| t[k].as_str().unwrap_or_default().to_string();
                        (s("name"), s("description"))
                    })
                    .collect(),
                name: item.name,
            })
            .collect()
    }

    /// Whether the session is given `run_code`: code mode is on and some server has a tool.
    fn code_on(&self, servers: &[say::Server]) -> bool {
        self.code_mode() && servers.iter().any(|s| !s.tools.is_empty())
    }

    /// The tool `name` means for the session: among the tools it can find now, else among what the
    /// servers give when asked again (a tool added since a card was written down).
    async fn named_tool(
        self: &Arc<Self>,
        run: &Arc<Run>,
        seat: &SessionKey,
        name: &str,
    ) -> (Option<Routed>, Vec<Routed>) {
        let mut tools = vec![];
        for (wait, fresh) in [(LIST_WAIT, false), (START_WAIT, true)] {
            tools = self.routed_tools(run, seat, tokio::time::Instant::now() + wait, fresh).await;
            if let Some(t) = find::named(&tools, name) {
                return (Some(t.clone()), tools);
            }
        }
        (None, tools)
    }

    /// Run one tool behind the router for the session in `seat`, until it answers, `CALL_WAIT`
    /// passes or `stop` is cancelled; recorded as `mcp.routed`, and in front of the lead when it
    /// calls one after handing out subtasks. A failure is the tool's error, which the model can act
    /// on.
    async fn call_routed(
        self: &Arc<Self>,
        run: &Arc<Run>,
        seat: &SessionKey,
        tool: &Routed,
        args: Value,
        stop: &CancellationToken,
    ) -> Value {
        let who = seat.who();
        let started = now_ms();
        let outcome = async {
            self.backend(run, &tool.item)
                .await?
                .request_until(
                    "tools/call",
                    json!({ "name": tool.tool, "arguments": args }),
                    CALL_WAIT,
                    stop.cancelled(),
                )
                .await
        }
        .await;
        // Who called which server's tool, how long it took and how it ended; never the arguments,
        // which may carry what the person typed or a credential.
        let e = ev(EventKind::McpRouted, who, format!("经 MCP 路由调用 {}", tool.name)).data(json!({
            "server": tool.item.name,
            "tool": tool.name,
            "ms": now_ms() - started,
            "error": outcome.as_ref().err().map(|e| format!("{e:#}")),
        }));
        self.emit(run, e.in_session(seat));
        // After handing out subtasks the lead only looks: a call it makes is put in front of it on
        // the board, once per tool, and goes on the delivery as a risk.
        // @project-doc docs/domains/20261031-goal_mode.md#lead_role
        let after_plan = *seat == SessionKey::Lead && {
            let mut st = run.st.lock().unwrap();
            !st.tasks.is_empty() && st.traces.record_lead_tool_after_plan(&tool.name)
        };
        if after_plan {
            self.emit(
                run,
                ev(EventKind::LeadRouted, "janus", format!("负责人派出子任务后调用了 {}", tool.name))
                    .to("lead")
                    .data(json!({ "tool": tool.name, "server": tool.item.name })),
            );
        }
        outcome.unwrap_or_else(|err| said(format!("{err:#}"), true))
    }

    /// One MCP request to the router from a session of this run (`seat` as for the board).
    pub async fn mcp_tools(
        self: &Arc<Self>,
        run_id: &str,
        token: &str,
        seat: &SessionKey,
        req: Value,
    ) -> Result<Option<Value>> {
        let run = self.session_run(run_id, token, seat)?;
        let Some(id) = req.get("id").cloned() else {
            if req["method"] == "notifications/cancelled" {
                run.router.cancel(seat, &req["params"]["requestId"]);
            }
            return Ok(None);
        };
        let lang = run.lang;
        let result = match req["method"].as_str().unwrap_or("") {
            "initialize" => {
                let servers = self.described(&run, seat, DESCRIBE_WAIT).await;
                let code = self.code_on(&servers);
                initialize(&req, crate::library::MCP_ROUTER, &say::instructions(&servers, lang, code))
            }
            "ping" => json!({}),
            "tools/list" => {
                let servers = self.described(&run, seat, LIST_WAIT).await;
                let mut tools = find::specs(say::find_description(&servers, lang), lang).to_vec();
                if self.code_on(&servers) {
                    tools.push(code::spec(&self.code_skills(&run, seat), lang));
                }
                json!({ "tools": tools })
            }
            "tools/call" => {
                // From now on the agent can cancel it, even while the tool is still looked up.
                let calling = run.router.calling(seat, &id);
                let args = &req["params"]["arguments"];
                match req["params"]["name"].as_str().unwrap_or_default() {
                    find::FIND => {
                        let tools = self.routed_tools(&run, seat, tokio::time::Instant::now() + LIST_WAIT, false).await;
                        let (text, failed) = find::find(&tools, args, lang);
                        said(text, failed)
                    }
                    code::CODE => self.run_code(&run, seat, args, &calling.stop).await,
                    find::CALL => {
                        let (name, inner) = find::call_args(args);
                        if name.is_empty() {
                            said(
                                tr!(
                                    lang,
                                    "call_tool 要写明调用哪个工具；find_tools 给出工具的名字。",
                                    "call_tool needs the name of the tool to run; find_tools gives the tools' names."
                                ),
                                true,
                            )
                        } else {
                            match self.named_tool(&run, seat, &name).await {
                                (Some(tool), _) => self.call_routed(&run, seat, &tool, inner, &calling.stop).await,
                                (None, tools) => said(find::no_such(&tools, &name, lang), true),
                            }
                        }
                    }
                    // A tool's own routed name still runs it, whatever was listed.
                    name => match self.named_tool(&run, seat, name).await {
                        (Some(tool), _) => self.call_routed(&run, seat, &tool, args.clone(), &calling.stop).await,
                        // A name nothing gives is a protocol error (MCP: unknown tool, -32602).
                        (None, tools) => {
                            return Ok(Some(rpc_error(id, INVALID_PARAMS, find::no_such(&tools, name, lang))));
                        }
                    },
                }
            }
            other => {
                let said = tr!(lang, "不支持 {other}", "{other} is not supported");
                return Ok(Some(rpc_error(id, METHOD_NOT_FOUND, said)));
            }
        };
        Ok(Some(rpc_result(id, result)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sweep stops a server left unused for `IDLE` and leaves one in use.
    #[tokio::test]
    async fn idle_servers_stop() {
        let dir = std::env::temp_dir().join(format!("janus-sweep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/fake_mcp.py");
        let server = McpServer::Stdio {
            command: "python3".into(),
            args: vec![script.to_string_lossy().into()],
            env_keys: vec![],
        };
        let (values, log) = (crate::library::McpValues::default(), dir.join("log"));
        let sandbox = crate::layout::Sandbox { home: dir.join("home"), tmp: dir.join("tmp") };
        let start = || Backend::start(&server, &values, &sandbox, &dir, || crate::acp::open_log(&log), START_WAIT);
        let router = McpRouter::default();
        let (idle, busy) = (start().await.unwrap(), start().await.unwrap());
        *router.slot("idle").try_lock().unwrap() = Some(idle.clone());
        *router.slot("busy").try_lock().unwrap() = Some(busy.clone());
        *idle.last_used.lock().unwrap() = std::time::Instant::now() - IDLE - Duration::from_secs(60);
        router.sweep();
        assert!(!idle.usable(), "an idle server stops");
        assert!(busy.usable(), "a server in use stays");
        router.close();
        assert!(!busy.usable());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
