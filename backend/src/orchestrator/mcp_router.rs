//! The MCP router (`janus-tools`): one MCP endpoint per run serving the library's command
//! servers for everyone, in place of handing each server to each session. A server starts the
//! first time a tool of it is called, in the run's working directory, and is shared by the run's
//! sessions; one idle for `IDLE` stops, and the rest go with the run. Listing the tools starts the
//! run's servers (in parallel) and asks each for its current list; a server's list is kept only
//! while that process runs, and asked again when it restarts or says its tools changed.
// @project-doc docs/domains/20261116-resource_library.md#mcp_router

use super::*;
use crate::library::{Item, McpServer};
use std::sync::atomic::{AtomicBool, AtomicU64};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin};
use tokio::sync::oneshot;

/// A server with no call for this long is stopped; the next call starts it again.
const IDLE: Duration = Duration::from_secs(10 * 60);
/// How long a server may take to start and answer `initialize` or `tools/list`.
const START_WAIT: Duration = Duration::from_secs(30);
/// How long one tool call may take.
const CALL_WAIT: Duration = Duration::from_secs(5 * 60);
/// What separates a server's name from its tool's in the names the router lists.
const SEP: &str = "__";

/// The run's servers, by item name. Each slot is locked while its server starts, so two sessions
/// asking at once start it once, while different servers start side by side.
#[derive(Default)]
pub(super) struct McpRouter {
    backends: tokio::sync::Mutex<HashMap<String, Arc<Slot>>>,
}

/// One server's place in the router: empty until it starts.
type Slot = tokio::sync::Mutex<Option<Arc<Backend>>>;

/// One server the router started: a command speaking MCP over stdio.
struct Backend {
    stdin: tokio::sync::Mutex<ChildStdin>,
    pending: Arc<StdMutex<HashMap<u64, oneshot::Sender<Value>>>>,
    next: AtomicU64,
    last_used: StdMutex<std::time::Instant>,
    /// The tools this process listed; cleared when it says they changed.
    tools: Arc<StdMutex<Option<Vec<Value>>>>,
    /// False once its output has closed: the process is gone and the next use starts it again.
    alive: Arc<AtomicBool>,
    /// Stopped with the backend (`kill_on_drop`).
    _child: Child,
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
    ) -> Result<Arc<Backend>> {
        let McpServer::Stdio { command, args, .. } = server else {
            bail!("MCP 路由只运行命令启动的服务器");
        };
        if let Some(dir) = log.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let err = std::fs::OpenOptions::new().create(true).append(true).open(log)?;
        let own = values.env.iter().filter(|(k, _)| !crate::agents::is_sandbox_var(k));
        let mut child = tokio::process::Command::new(command)
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
            last_used: StdMutex::new(std::time::Instant::now()),
            tools: tools.clone(),
            alive: alive.clone(),
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
        backend.request("initialize", init, START_WAIT).await?;
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

    /// Still running, and used recently enough to keep.
    fn usable(&self) -> bool {
        self.alive.load(Ordering::SeqCst) && self.last_used.lock().unwrap().elapsed() < IDLE
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
        *self.last_used.lock().unwrap() = std::time::Instant::now();
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

/// A tool's name as the router lists it: `<server>__<tool>`, in the characters MCP clients take,
/// at most 64 of them.
fn routed_name(server: &str, tool: &str) -> String {
    let clean = |s: &str| {
        s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect::<String>()
    };
    let mut name = format!("{}{SEP}{}", clean(server), clean(tool));
    name.truncate(64);
    name
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

    /// The servers the router serves for this run: the command servers for everyone in its
    /// members' snapshots, by name.
    fn routed_servers(run: &Run) -> BTreeMap<String, Item> {
        run.team
            .iter()
            .flat_map(|m| m.resources.iter().filter(|i| crate::library::routed_server(i)))
            .map(|i| (i.name.clone(), i.clone()))
            .collect()
    }

    /// A running backend for `item`: the one already started, unless it exited or sat idle too
    /// long, in which case it starts again (and lists its tools afresh).
    async fn backend(&self, run: &Run, item: &Item) -> Result<Arc<Backend>> {
        let slot = run.router.backends.lock().await.entry(item.name.clone()).or_default().clone();
        let mut slot = slot.lock().await;
        if let Some(b) = slot.as_ref().filter(|b| b.usable()) {
            return Ok(b.clone());
        }
        // Dropping the old one stops its process.
        *slot = None;
        let crate::library::Body::Mcp { server } = &item.body else { bail!("{} 不是 MCP 服务器", item.name) };
        let log = self.cfg.data_dir.join("logs").join(&run.id).join(format!("mcp-{}.log", item.name));
        let b = Backend::start(server, &self.mcp_values(&item.id), &self.server_home(&item.id), &run.cwd, &log).await?;
        *slot = Some(b.clone());
        Ok(b)
    }

    async fn server_tools(&self, run: &Run, item: &Item) -> Result<Vec<Value>> {
        self.backend(run, item).await?.tools().await
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
                        "Janus 资源库中给所有数字员工的 MCP 服务器，经 Janus 路由：{}。工具名是「服务器{SEP}工具」；服务器在第一次调用时启动，工作目录是这个任务的工作目录。",
                        names.join("、")
                    ),
                })
            }
            "ping" => json!({}),
            "tools/list" => {
                // Every server starts and lists at once; one that fails is left out and logged.
                let mut listing = tokio::task::JoinSet::new();
                for item in servers.values().cloned() {
                    let (app, run) = (self.clone(), run.clone());
                    listing.spawn(async move {
                        let got = app.server_tools(&run, &item).await;
                        (item.name, got)
                    });
                }
                let mut by_server = BTreeMap::new();
                while let Some(Ok((name, got))) = listing.join_next().await {
                    match got {
                        Ok(list) => {
                            by_server.insert(name, list);
                        }
                        Err(err) => crate::config::log(format!("janus: MCP 路由列不出 {name} 的工具：{err:#}")),
                    }
                }
                let tools: Vec<Value> = by_server
                    .into_iter()
                    .flat_map(|(server, list)| {
                        list.into_iter().map(move |mut t| {
                            t["name"] = json!(routed_name(&server, t["name"].as_str().unwrap_or_default()));
                            t
                        })
                    })
                    .collect();
                json!({ "tools": tools })
            }
            "tools/call" => {
                let name = req["params"]["name"].as_str().unwrap_or_default();
                let started = now_ms();
                let found = servers.values().find_map(|item| {
                    let prefix = routed_name(&item.name, "");
                    name.strip_prefix(&prefix).map(|_| item.clone())
                });
                let outcome: Result<Value> = async {
                    let item = found.as_ref().with_context(|| format!("没有这个工具：{name}"))?;
                    let tool = self
                        .server_tools(&run, item)
                        .await?
                        .into_iter()
                        .filter_map(|t| t["name"].as_str().map(str::to_owned))
                        .find(|t| routed_name(&item.name, t) == name)
                        .with_context(|| format!("{} 没有这个工具：{name}", item.name))?;
                    let args = req["params"]["arguments"].clone();
                    self.backend(&run, item)
                        .await?
                        .request("tools/call", json!({ "name": tool, "arguments": args }), CALL_WAIT)
                        .await
                }
                .await;
                // Who called which server's tool, how long it took and how it ended; never the
                // arguments, which may carry what the person typed or a credential.
                let mut e = ev("mcp.routed", who, format!("经 MCP 路由调用 {name}")).data(json!({
                    "server": found.as_ref().map(|i| i.name.clone()),
                    "tool": name,
                    "ms": now_ms() - started,
                    "error": outcome.as_ref().err().map(|e| format!("{e:#}")),
                }));
                if who != "lead" {
                    e = e.task(who);
                }
                self.emit(&run, e);
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
