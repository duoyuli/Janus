//! What Janus's two MCP servers for a run's sessions share: the shared board (`board`) and the
//! library router (`mcp_router`). Each session reaches them at a URL that carries a token derived
//! from the run's secret and the session's `who`, so it opens them only as itself; both speak
//! streamable HTTP with JSON responses and wrap their tools in the same JSON-RPC envelope.
// @project-doc docs/interfaces/20261031-board_mcp.md#transport

use super::mcp_client::agreed_version;
use super::*;

/// JSON-RPC: the method does not exist.
pub(super) const METHOD_NOT_FOUND: i64 = -32601;
/// JSON-RPC: the params are wrong; MCP uses it for a tool no server has.
pub(super) const INVALID_PARAMS: i64 = -32602;

/// The token in one session's MCP URL: derived from the run's secret and the session's `who`, so
/// it only opens the board as that member.
pub(super) fn session_token(run_token: &str, who: &str) -> String {
    Sha256::digest(format!("{run_token}\0{who}").as_bytes())[..16].iter().map(|x| format!("{x:02x}")).collect()
}

/// The run's board secret, from the system's random source. Without one there is no secret
/// worth the name, so the session does not start (AT-18).
pub(super) fn random_token() -> Result<String> {
    use std::io::Read;
    let mut b = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .context("读取系统随机数失败，无法生成看板令牌")?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

/// The answer to `initialize`: the protocol version both sides speak, tools only, and what the
/// server is for.
pub(super) fn initialize(req: &Value, name: &str, instructions: &str) -> Value {
    json!({
        "protocolVersion": agreed_version(&req["params"]["protocolVersion"]),
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": name, "version": crate::VERSION },
        "instructions": instructions,
    })
}

/// The response to request `id`.
pub(super) fn rpc_result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// The error response to request `id`.
pub(super) fn rpc_error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message.into() } })
}

/// A tool's answer as text; `failed` marks the tool's own error, which the model can act on.
pub(super) fn tool_text(text: &str, failed: bool) -> Value {
    let mut out = json!({ "content": [{ "type": "text", "text": text }] });
    if failed {
        out["isError"] = json!(true);
    }
    out
}

impl App {
    /// The entry a session of `who` is given for one of the run's servers: `name` as the agent
    /// lists it, at `/mcp/<run>/<token>/<who><path>`.
    pub(super) fn session_server(&self, run: &Run, who: &str, name: &str, path: &str) -> Value {
        let token = session_token(&run.st.lock().unwrap().mcp_token, who);
        let url = format!("http://{}/mcp/{}/{}/{}{path}", self.cfg.addr, run.id, token, who);
        json!({ "type": "http", "name": name, "url": url, "headers": [] })
    }

    /// The live run a request to its MCP servers comes from, when the URL's token is the one Janus
    /// gave the session of `who`. The token is bound to its `who`, so a collaborator cannot rename
    /// itself to `lead` or to another task: only the session Janus started holds the token for that
    /// name.
    pub(super) fn session_run(&self, run_id: &str, token: &str, who: &str) -> Result<Arc<Run>> {
        let run = self.run(run_id).context("这个任务不在运行")?;
        let run_token = run.st.lock().unwrap().mcp_token.clone();
        if run_token.is_empty() || token != session_token(&run_token, who) {
            bail!("token 不对");
        }
        Ok(run)
    }
}

impl Ev {
    /// The event happens in the session of `who` as the MCP URL names it: a collaborator's is its
    /// task's, the lead's belongs to no task.
    pub(super) fn in_session(self, who: &str) -> Ev {
        if who == "lead" { self } else { self.task(who) }
    }
}
