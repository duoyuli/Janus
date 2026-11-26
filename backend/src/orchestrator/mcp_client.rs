//! The MCP router's connection to one library server, whatever its transport: a command it starts
//! and talks to over stdio, a Streamable HTTP endpoint, or an endpoint of the older SSE transport.
//! Every transport ends in the same inbox: a reply reaches whoever waits on its id, the server's own
//! requests (`roots/list`, `ping`) are answered here, and `notifications/tools/list_changed` drops
//! the tools it listed. A request the router stops waiting for is cancelled on the server.
// @project-doc docs/domains/20261116-resource_library.md#mcp_router

use super::*;
use crate::library::{McpServer, McpValues};
use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use std::sync::atomic::AtomicU64;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin};

/// How long reaching a URL server may take before a request gives up on it.
const CONNECT_WAIT: Duration = Duration::from_secs(15);
/// The session header of Streamable HTTP.
const SESSION: &str = "mcp-session-id";
/// The header that names the protocol version agreed in `initialize`.
const VERSION: &str = "mcp-protocol-version";
/// The MCP versions Janus speaks, newest first: what the router and the board agree to with an
/// agent, and what the router accepts from a library server. Every one has `initialize` and the
/// same `tools/list` and `tools/call`; the stateless 2026-07-28 revision, which has neither
/// `initialize` nor sessions, is not among them.
const VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
/// At most this many pages of `tools/list` are read from one server.
const LIST_PAGES: usize = 100;
/// How long telling a server to stop a request may take; it is told and not waited on beyond that.
const CANCEL_WAIT: Duration = Duration::from_secs(5);

/// The version to answer a client's `initialize` with: the one it asked for when Janus speaks it,
/// otherwise the newest Janus speaks, and the client decides whether to go on.
pub(super) fn agreed_version(asked: &Value) -> &'static str {
    VERSIONS.into_iter().find(|v| asked.as_str() == Some(*v)).unwrap_or(VERSIONS[0])
}

/// One server the router reaches.
pub(super) struct Backend {
    link: Link,
    inbox: Arc<Inbox>,
    next: AtomicU64,
    pub(super) last_used: StdMutex<std::time::Instant>,
    /// What the server said of itself in `initialize`.
    pub(super) said: StdMutex<Said>,
}

/// What a server says of itself in its answer to `initialize`: its title (`serverInfo.title`) and
/// one line of what it is for, its `serverInfo.description` or else its `instructions`' first
/// line. The rest of its instructions is not kept: the router's own instructions name every server
/// in a budget, and how each tool is used is in its own description.
// @project-doc docs/domains/20261116-resource_library.md#mcp_router
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Said {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub says: String,
}

/// How much of a server's title and of what it says it is for is kept.
const SAID_TITLE: usize = 80;
const SAID_SAYS: usize = 300;

impl Said {
    /// What the answer to `initialize` says of its server.
    pub(super) fn of(init: &Value) -> Said {
        let info = &init["serverInfo"];
        let described = info["description"].as_str().unwrap_or_default().trim();
        let says = if described.is_empty() {
            init["instructions"]
                .as_str()
                .unwrap_or_default()
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty())
                .unwrap_or("")
        } else {
            described
        };
        Said {
            title: crate::text::one_line(info["title"].as_str().unwrap_or_default(), SAID_TITLE),
            says: crate::text::one_line(says, SAID_SAYS),
        }
    }
}

/// How messages reach the server.
enum Link {
    /// A command the router started, in its own process group (`process_group(0)`) so what it
    /// spawned (`npx` runs the real server as a child) ends with it. Messages are lines on stdin.
    Stdio {
        stdin: tokio::sync::Mutex<ChildStdin>,
        group: Option<u32>,
        /// Killed with the backend (`kill_on_drop`) if the group's TERM did not end it.
        _child: Child,
    },
    /// Streamable HTTP: each message is a POST, answered with JSON or with an event stream.
    Http { web: Web, session: StdMutex<Option<String>>, version: StdMutex<Option<String>> },
    /// The older SSE transport: replies come on the event stream the router keeps open; messages go
    /// by POST to the endpoint the stream named, on the server's own origin.
    Sse { web: Web, endpoint: reqwest::Url, stream: tokio::task::AbortHandle },
}

/// A URL server's address and the headers saved on it.
struct Web {
    client: reqwest::Client,
    url: reqwest::Url,
    headers: HeaderMap,
}

impl Web {
    fn new(url: &str, values: Vec<(String, String)>) -> Result<Web> {
        let url = reqwest::Url::parse(url).with_context(|| format!("地址不对：{url}"))?;
        if !matches!(url.scheme(), "http" | "https") {
            bail!("只支持 http 和 https 地址：{url}");
        }
        let mut headers = HeaderMap::new();
        for (k, v) in values {
            let name = HeaderName::from_bytes(k.as_bytes()).with_context(|| format!("header 名不对：{k}"))?;
            let mut value = HeaderValue::from_str(&v).with_context(|| format!("header {k} 的值不对"))?;
            value.set_sensitive(true);
            headers.insert(name, value);
        }
        let client = reqwest::Client::builder().connect_timeout(CONNECT_WAIT).build()?;
        Ok(Web { client, url, headers })
    }
}

/// A started server's life: running; ended on its own (its process exited, its stream closed, its
/// HTTP session expired), so the next use starts it again; or stopped by Janus, after which nothing
/// uses it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Life {
    Running,
    Exited,
    Stopped,
}

/// Where every transport delivers what the server says.
struct Inbox {
    pending: StdMutex<HashMap<u64, oneshot::Sender<Value>>>,
    /// The tools this server listed; cleared when it says they changed.
    tools: StdMutex<Option<Vec<Value>>>,
    life: StdMutex<Life>,
    /// The server's own requests, answered by the backend.
    requests: mpsc::UnboundedSender<Value>,
}

impl Inbox {
    fn receive(&self, msg: Value) {
        if msg.get("method").is_some() {
            if msg.get("id").is_some() {
                let _ = self.requests.send(msg);
            } else if msg["method"] == "notifications/tools/list_changed" {
                *self.tools.lock().unwrap() = None;
            }
            return;
        }
        if let Some(id) = msg["id"].as_u64()
            && let Some(tx) = self.pending.lock().unwrap().remove(&id)
        {
            let _ = tx.send(msg);
        }
    }

    /// The server is gone: whoever still waits gets an error instead of hanging.
    fn ended(&self) {
        {
            let mut life = self.life.lock().unwrap();
            if *life == Life::Running {
                *life = Life::Exited;
            }
        }
        self.pending.lock().unwrap().clear();
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Backend {
    /// Start or reach `server` and initialize it. A command runs in `cwd`, in a sandbox of its own
    /// (`sandbox_vars(sandbox)`), never in Janus's environment: it sees the sandbox variables and the
    /// values saved on it, which can't override them; its stderr goes to the file `log` opens. A URL gets the
    /// headers saved on it, from this process. `roots/list` from the server is answered with `cwd`.
    // @project-doc docs/operations/20261031-security.md#library
    pub(super) async fn start(
        server: &McpServer,
        values: &McpValues,
        sandbox: &crate::layout::Sandbox,
        cwd: &Path,
        log: impl FnOnce() -> Result<std::fs::File>,
        wait: Duration,
    ) -> Result<Arc<Backend>> {
        let (requests_tx, mut requests) = mpsc::unbounded_channel::<Value>();
        let inbox = Arc::new(Inbox {
            pending: StdMutex::default(),
            tools: StdMutex::default(),
            life: StdMutex::new(Life::Running),
            requests: requests_tx,
        });
        let set = server.set_values(values);
        let link = match server {
            McpServer::Stdio { command, args, .. } => {
                let err = log()?;
                let own = set.into_iter().filter(|(k, _)| !crate::agents::is_sandbox_var(k));
                let mut cmd = tokio::process::Command::new(command);
                #[cfg(unix)]
                cmd.process_group(0);
                let mut child = cmd
                    .args(args)
                    .env_clear()
                    .envs(crate::agents::sandbox_vars(sandbox)?)
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
                let reading = inbox.clone();
                tokio::spawn(async move {
                    let mut lines = BufReader::new(stdout).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        if let Ok(msg) = serde_json::from_str::<Value>(&line) {
                            reading.receive(msg);
                        }
                    }
                    reading.ended();
                });
                Link::Stdio { stdin: tokio::sync::Mutex::new(stdin), group: child.id(), _child: child }
            }
            McpServer::Http { url, .. } => {
                Link::Http { web: Web::new(url, set)?, session: StdMutex::default(), version: StdMutex::default() }
            }
            McpServer::Sse { url, .. } => {
                let web = Web::new(url, set)?;
                let (endpoint, stream) = open_sse(&web, inbox.clone(), wait).await?;
                Link::Sse { web, endpoint, stream }
            }
        };
        let backend = Arc::new(Backend {
            link,
            inbox,
            next: AtomicU64::new(1),
            last_used: StdMutex::new(std::time::Instant::now()),
            said: StdMutex::default(),
        });
        let root = format!("file://{}", cwd.display());
        let answering = Arc::downgrade(&backend);
        tokio::spawn(async move {
            while let Some(req) = requests.recv().await {
                let Some(b) = answering.upgrade() else { break };
                let id = req["id"].clone();
                let reply = match req["method"].as_str() {
                    Some("roots/list") => rpc_result(id, json!({ "roots": [{ "uri": root, "name": "workspace" }] })),
                    Some("ping") => rpc_result(id, json!({})),
                    _ => rpc_error(id, METHOD_NOT_FOUND, "Janus 不支持这个请求"),
                };
                let _ = b.send(&reply).await;
            }
        });
        let init = json!({
            "protocolVersion": VERSIONS[0],
            "capabilities": { "roots": { "listChanged": false } },
            "clientInfo": { "name": "janus", "version": crate::VERSION }
        });
        let answer = backend.request("initialize", init, wait).await?;
        *backend.said.lock().unwrap() = Said::of(&answer);
        let agreed = answer["protocolVersion"].as_str().unwrap_or_default();
        if !VERSIONS.contains(&agreed) {
            bail!("服务器要用 MCP 协议版本「{agreed}」，Janus 支持的是 {}", VERSIONS.join("、"));
        }
        if let Link::Http { version, .. } = &backend.link {
            *version.lock().unwrap() = Some(agreed.to_string());
        }
        backend.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).await?;
        Ok(backend)
    }

    /// Its tools: the list it gave, or asked now, every page of it (`nextCursor`), all within `wait`.
    pub(super) async fn tools(&self, wait: Duration) -> Result<Vec<Value>> {
        if let Some(t) = self.inbox.tools.lock().unwrap().clone() {
            return Ok(t);
        }
        let until = tokio::time::Instant::now() + wait;
        let mut tools = vec![];
        let mut cursor: Option<String> = None;
        for _ in 0..LIST_PAGES {
            let params = cursor.as_ref().map_or_else(|| json!({}), |c| json!({ "cursor": c }));
            let left = until.saturating_duration_since(tokio::time::Instant::now());
            let page = self.request("tools/list", params, left).await?;
            tools.extend(page["tools"].as_array().cloned().unwrap_or_default());
            match page["nextCursor"].as_str() {
                // A server handing back the cursor it was given would never end.
                Some(next) if cursor.as_deref() != Some(next) => cursor = Some(next.to_string()),
                _ => break,
            }
        }
        *self.inbox.tools.lock().unwrap() = Some(tools.clone());
        Ok(tools)
    }

    pub(super) fn usable(&self) -> bool {
        *self.inbox.life.lock().unwrap() == Life::Running
    }

    pub(super) fn idle_for(&self) -> Duration {
        self.last_used.lock().unwrap().elapsed()
    }

    /// Let it go; a call still waiting on it gets an error. A command's process group is ended:
    /// TERM now, KILL a few seconds later for whatever ignored it, even when the server itself
    /// already exited, since what it spawned may not have. A Streamable HTTP session is ended on the
    /// server; an SSE stream is closed.
    pub(super) fn stop(&self) {
        if std::mem::replace(&mut *self.inbox.life.lock().unwrap(), Life::Stopped) == Life::Stopped {
            return;
        }
        self.inbox.pending.lock().unwrap().clear();
        match &self.link {
            Link::Stdio { group, .. } => {
                if let Some(pid) = *group {
                    crate::acp::signal_group(pid, crate::acp::Signal::Term);
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_secs(3));
                        crate::acp::signal_group(pid, crate::acp::Signal::Kill);
                    });
                }
            }
            Link::Http { web, session, .. } => {
                if let Some(id) = session.lock().unwrap().take()
                    && let Ok(rt) = tokio::runtime::Handle::try_current()
                {
                    let end = web.client.delete(web.url.clone()).headers(web.headers.clone()).header(SESSION, id);
                    rt.spawn(async move {
                        let _ = end.send().await;
                    });
                }
            }
            Link::Sse { stream, .. } => stream.abort(),
        }
    }

    async fn send(&self, msg: &Value) -> Result<()> {
        match &self.link {
            Link::Stdio { stdin, .. } => {
                let mut line = serde_json::to_vec(msg)?;
                line.push(b'\n');
                let mut stdin = stdin.lock().await;
                stdin.write_all(&line).await?;
                stdin.flush().await?;
                Ok(())
            }
            Link::Http { web, session, version } => self.post_http(web, session, version, msg).await,
            Link::Sse { web, endpoint, .. } => {
                let resp = web
                    .client
                    .post(endpoint.clone())
                    .headers(web.headers.clone())
                    .header(CONTENT_TYPE, "application/json")
                    .body(serde_json::to_vec(msg)?)
                    .send()
                    .await
                    .context("连不上服务器")?;
                refused(resp).await.map(|_| ())
            }
        }
    }

    /// One message over Streamable HTTP. The reply comes back as JSON (one message or a batch) or
    /// as an event stream read in the background; either way into the inbox. The session the
    /// server gives in `initialize` goes on every later message; a session it no longer knows
    /// (404) ends this backend, so the next use initializes again.
    async fn post_http(
        &self,
        web: &Web,
        session: &StdMutex<Option<String>>,
        version: &StdMutex<Option<String>>,
        msg: &Value,
    ) -> Result<()> {
        let mut req = web
            .client
            .post(web.url.clone())
            .headers(web.headers.clone())
            .header(ACCEPT, "application/json, text/event-stream")
            .header(CONTENT_TYPE, "application/json")
            .body(serde_json::to_vec(msg)?);
        let held = session.lock().unwrap().clone();
        if let Some(id) = &held {
            req = req.header(SESSION, id);
        }
        if let Some(v) = version.lock().unwrap().clone() {
            req = req.header(VERSION, v);
        }
        let resp = req.send().await.context("连不上服务器")?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND && held.is_some() {
            self.inbox.ended();
            bail!("服务器结束了这个会话，下次调用时重新连接");
        }
        let resp = refused(resp).await?;
        if let Some(id) = resp.headers().get(SESSION).and_then(|v| v.to_str().ok()) {
            *session.lock().unwrap() = Some(id.to_string());
        }
        let stream = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|t| t.starts_with("text/event-stream"));
        if stream {
            let inbox = self.inbox.clone();
            tokio::spawn(async move {
                let mut events = Events::new(resp);
                while let Ok(Some(e)) = events.next().await {
                    if let Ok(msg) = serde_json::from_str::<Value>(&e.data) {
                        inbox.receive(msg);
                    }
                }
            });
            return Ok(());
        }
        let body = resp.bytes().await?;
        if !body.is_empty() {
            match serde_json::from_slice::<Value>(&body).context("服务器的回应不是 JSON")? {
                Value::Array(all) => all.into_iter().for_each(|m| self.inbox.receive(m)),
                one => self.inbox.receive(one),
            }
        }
        Ok(())
    }

    /// One request; its result, or the server's error as an error. `wait` covers sending it too:
    /// over HTTP the reply may come with the response.
    pub(super) async fn request(&self, method: &str, params: Value, wait: Duration) -> Result<Value> {
        self.request_until(method, params, wait, std::future::pending()).await
    }

    /// A request the caller may give up on early (`cancel`). Whenever the wait ends without a
    /// reply, out of time or cancelled, the server is told (`notifications/cancelled`), so it can
    /// stop the work instead of finishing it for no one; `initialize` is never cancelled, as MCP
    /// requires.
    pub(super) async fn request_until(
        &self,
        method: &str,
        params: Value,
        wait: Duration,
        cancel: impl std::future::Future<Output = ()>,
    ) -> Result<Value> {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.inbox.pending.lock().unwrap().insert(id, tx);
        *self.last_used.lock().unwrap() = std::time::Instant::now();
        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let asked = tokio::time::timeout(wait, async {
            self.send(&msg).await?;
            rx.await.map_err(|_| anyhow!("服务器已经退出"))
        });
        // A cancel that came before the request was sent wins: the request is not sent at all.
        let (reason, err) = tokio::select! {
            biased;
            () = cancel => ("cancelled by the client", anyhow!("调用已取消")),
            asked = asked => match asked {
                Ok(Ok(reply)) => return Self::result(reply),
                Ok(Err(err)) => {
                    self.inbox.pending.lock().unwrap().remove(&id);
                    return Err(err);
                }
                Err(_) => ("timed out", anyhow!("{} 秒内没有回应", wait.as_secs())),
            },
        };
        self.inbox.pending.lock().unwrap().remove(&id);
        if method != "initialize" && self.usable() {
            let cancelled = json!({
                "jsonrpc": "2.0",
                "method": "notifications/cancelled",
                "params": { "requestId": id, "reason": reason }
            });
            let _ = tokio::time::timeout(CANCEL_WAIT, self.send(&cancelled)).await;
        }
        Err(err)
    }

    /// A reply's result, or its error as an error.
    fn result(reply: Value) -> Result<Value> {
        if let Some(err) = reply.get("error") {
            bail!("{}", err["message"].as_str().unwrap_or("服务器返回了错误"));
        }
        Ok(reply["result"].clone())
    }
}

/// A response that is not a success, as an error with the start of its body.
async fn refused(resp: reqwest::Response) -> Result<reqwest::Response> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let body = resp.text().await.unwrap_or_default();
    bail!("服务器返回 HTTP {status}：{}", clip(body.trim(), 200))
}

/// Open the SSE transport's stream and wait for the endpoint it names for messages. The endpoint
/// must be on the server's own origin: the saved headers go with every message. Later events go to
/// the inbox; the stream closing ends the backend.
async fn open_sse(web: &Web, inbox: Arc<Inbox>, wait: Duration) -> Result<(reqwest::Url, tokio::task::AbortHandle)> {
    let resp = web
        .client
        .get(web.url.clone())
        .headers(web.headers.clone())
        .header(ACCEPT, "text/event-stream")
        .send()
        .await
        .context("连不上服务器")?;
    let resp = refused(resp).await?;
    let (tx, rx) = oneshot::channel();
    let base = web.url.clone();
    let reading = tokio::spawn(async move {
        let mut named = Some(tx);
        let mut events = Events::new(resp);
        while let Ok(Some(e)) = events.next().await {
            if e.event == "endpoint" {
                if let Some(tx) = named.take() {
                    let _ = tx.send(base.join(e.data.trim()));
                }
            } else if let Ok(msg) = serde_json::from_str::<Value>(&e.data) {
                inbox.receive(msg);
            }
        }
        inbox.ended();
    });
    let stream = reading.abort_handle();
    let named = tokio::time::timeout(wait, rx).await;
    let endpoint = match named {
        Ok(Ok(Ok(url))) if url.origin() == web.url.origin() => url,
        Ok(Ok(Ok(url))) => {
            stream.abort();
            bail!("SSE 服务器给的消息地址不在它自己的域名下：{url}")
        }
        Ok(Ok(Err(err))) => {
            stream.abort();
            bail!("SSE 服务器给的消息地址不对：{err}")
        }
        Ok(Err(_)) => bail!("SSE 服务器没有给出消息地址就关闭了连接"),
        Err(_) => {
            stream.abort();
            bail!("SSE 服务器 {} 秒内没有给出消息地址", wait.as_secs())
        }
    };
    Ok((endpoint, stream))
}

/// One server-sent event.
#[derive(Debug, PartialEq)]
struct Event {
    event: String,
    data: String,
}

/// The events of a response body, as they arrive.
struct Events {
    resp: reqwest::Response,
    buf: Vec<u8>,
}

impl Events {
    fn new(resp: reqwest::Response) -> Events {
        Events { resp, buf: vec![] }
    }

    async fn next(&mut self) -> Result<Option<Event>> {
        let (mut event, mut data) = (String::new(), Vec::<String>::new());
        loop {
            while let Some(end) = self.buf.iter().position(|b| *b == b'\n') {
                let raw: Vec<u8> = self.buf.drain(..=end).collect();
                if let Some(e) = event_line(&String::from_utf8_lossy(&raw), &mut event, &mut data) {
                    return Ok(Some(e));
                }
            }
            match self.resp.chunk().await? {
                Some(chunk) => self.buf.extend_from_slice(&chunk),
                None => return Ok(None),
            }
        }
    }
}

/// Take one line of an event stream; a blank line ends the event, if it carried data.
fn event_line(raw: &str, event: &mut String, data: &mut Vec<String>) -> Option<Event> {
    let line = raw.trim_end_matches(['\n', '\r']);
    if line.is_empty() {
        let done = (!data.is_empty()).then(|| Event {
            event: if event.is_empty() { "message".into() } else { event.clone() },
            data: data.join("\n"),
        });
        event.clear();
        data.clear();
        return done;
    }
    if line.starts_with(':') {
        return None;
    }
    let (field, value) = line.split_once(':').map(|(f, v)| (f, v.strip_prefix(' ').unwrap_or(v))).unwrap_or((line, ""));
    match field {
        "event" => *event = value.to_string(),
        "data" => data.push(value.to_string()),
        _ => {}
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Events end at a blank line; comments and unknown fields are skipped, data lines join, and a
    /// line may end in CRLF.
    #[test]
    fn event_streams_split_into_events() {
        let (mut event, mut data) = (String::new(), vec![]);
        let mut out = vec![];
        for line in [
            ": hi\n",
            "event: endpoint\r\n",
            "data: /messages?s=1\n",
            "\n",
            "id: 7\n",
            "data: {\"a\":\n",
            "data: 1}\n",
            "\n",
            "\n",
        ] {
            out.extend(event_line(line, &mut event, &mut data));
        }
        assert_eq!(
            out,
            [
                Event { event: "endpoint".into(), data: "/messages?s=1".into() },
                Event { event: "message".into(), data: "{\"a\":\n1}".into() },
            ]
        );
    }
}
