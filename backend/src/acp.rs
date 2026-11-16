//! ACP client built on the official `agent-client-protocol` SDK: JSON-RPC 2.0 over the agent's
//! stdio, one JSON object per line. Janus is the ACP client; the ACP adapters and custom
//! commands are the agents.
//!
//! The SDK owns framing, request ids, response routing and cancellation. Janus still starts the
//! subprocess itself, because an agent must see only `RuntimeSpec.env` (nothing inherited), run in
//! the Run's workspace and write stderr to its own log file.

use agent_client_protocol::{
    Agent, ByteStreams, Client, Error as AcpError, RawJsonRpcId, Responder, UntypedMessage, V1,
    is_connection_terminated, is_incoming_transport_closed,
};
use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex as StdMutex};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

const ACP_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30 * 60);

/// The wire id of an incoming ACP request, as the SDK saw it.
pub type RequestId = RawJsonRpcId;

#[derive(Clone, Debug)]
pub struct RuntimeSpec {
    pub id: String,
    /// Decides adapter-specific session settings.
    pub kind: crate::agents::Runtime,
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
    /// The complete environment. Nothing is inherited from Janus's own process: an agent sees
    /// only these variables, so the user's shell setup, credentials and agent settings cannot leak in.
    pub env: Vec<(String, String)>,
    /// The adapter version the session runs from, kept installed while the session lives.
    pub pin: Option<Arc<crate::adapters::VersionPin>>,
}

pub enum Incoming {
    Notification { method: String, params: Value },
    Request { id: RequestId, method: String, params: Value },
    Closed,
}

/// Responses are sent by the orchestrator later, so the SDK's responder is parked here until then.
type Pending = Arc<StdMutex<HashMap<RequestId, Responder<Value, V1>>>>;
type Connection = agent_client_protocol::v1::ConnectionTo<Agent>;

pub struct AcpConn {
    cx: Connection,
    child: Mutex<Child>,
    pending: Pending,
    task: StdMutex<Option<tokio::task::JoinHandle<()>>>,
    pub agent_info: Value,
    pub capabilities: Value,
    /// The agent accepts `_session/steering` mid-turn (top-level `_meta.steering`).
    pub steering: bool,
    /// Keeps the adapter version this session was started from (`RuntimeSpec::pin`).
    _pin: Option<Arc<crate::adapters::VersionPin>>,
}

impl AcpConn {
    // @project-doc docs/interfaces/acp_integration.md#transport
    pub async fn spawn(
        spec: &RuntimeSpec,
        cwd: &Path,
        stderr_log: &Path,
    ) -> Result<(Arc<AcpConn>, mpsc::UnboundedReceiver<Incoming>)> {
        if let Some(dir) = stderr_log.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let log = std::fs::OpenOptions::new().create(true).append(true).open(stderr_log)?;
        let mut cmd = Command::new(&spec.program);
        cmd.env_clear();
        #[cfg(unix)]
        {
            cmd.process_group(0);
        }
        let mut child = cmd
            .args(&spec.args)
            .envs(spec.env.iter().cloned())
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(log))
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("启动 {} 失败", spec.name))?;
        let stdin = child.stdin.take().context("no stdin")?;
        let stdout = child.stdout.take().context("no stdout")?;

        let pending: Pending = Arc::default();
        let (tx, rx) = mpsc::unbounded_channel();
        let (ready_tx, ready_rx) = oneshot::channel();
        let transport = ByteStreams::new(stdin.compat_write(), stdout.compat());
        let request_tx = tx.clone();
        let notification_tx = tx.clone();
        let handler_pending = pending.clone();
        let task = tokio::spawn(async move {
            let result = Client
                .v1()
                .name("janus")
                .on_receive_notification(
                    async move |msg: UntypedMessage, _cx| {
                        let _ = notification_tx.send(Incoming::Notification { method: msg.method, params: msg.params });
                        Ok::<(), AcpError>(())
                    },
                    agent_client_protocol::on_receive_notification!(),
                )
                .on_receive_request(
                    async move |msg: UntypedMessage, responder: Responder<Value, V1>, _cx| {
                        // The person may answer much later, so hand the responder to the
                        // orchestrator instead of answering inside the dispatch loop.
                        let id = responder.raw_id().clone();
                        handler_pending.lock().unwrap().insert(id.clone(), responder);
                        if request_tx
                            .send(Incoming::Request { id: id.clone(), method: msg.method, params: msg.params })
                            .is_err()
                        {
                            handler_pending.lock().unwrap().remove(&id);
                        }
                        Ok::<(), AcpError>(())
                    },
                    agent_client_protocol::on_receive_request!(),
                )
                .connect_with(transport, async move |cx| {
                    let init = cx
                        .send_request(UntypedMessage::new(
                            "initialize",
                            json!({
                                "protocolVersion": 1,
                                // Forms and links an agent asks the person to fill in or open (AskUserQuestion, MCP input,
                                // plan feedback) are rendered in the inbox next to permission requests.
                                "clientCapabilities": {
                                    "fs": { "readTextFile": false, "writeTextFile": false },
                                    "terminal": false,
                                    "elicitation": { "form": {}, "url": {} },
                                    // AIR clients get the goal extension and the AIR tool-call report.
                                    "_meta": { "jetbrains": { "air": {} } }
                                },
                                "clientInfo": { "name": "janus", "version": env!("CARGO_PKG_VERSION") }
                            }),
                        )?)
                        .block_task()
                        .await;
                    match init {
                        Ok(response) => {
                            let _ = ready_tx.send(Ok((cx, response)));
                            // The connection lives until the orchestrator kills the session.
                            std::future::pending::<()>().await;
                            Ok(())
                        }
                        Err(err) => {
                            let _ = ready_tx.send(Err(err.to_string()));
                            Err(err)
                        }
                    }
                })
                .await;
            // Every way the session ends reaches the orchestrator as Incoming::Closed; a request
            // still waiting on the connection fails with the SDK's transport error.
            drop(result);
            let _ = tx.send(Incoming::Closed);
        });

        let (cx, init) = match tokio::time::timeout(ACP_REQUEST_TIMEOUT, ready_rx).await {
            Ok(Ok(Ok(ready))) => ready,
            Ok(Ok(Err(message))) => {
                task.abort();
                bail!("{} 的 ACP initialize 失败：{message}", spec.name);
            }
            Ok(Err(_)) => {
                task.abort();
                bail!("{} 的 ACP initialize 失败：连接已结束", spec.name);
            }
            Err(_) => {
                task.abort();
                bail!("{} 的 ACP initialize 失败：超过 {} 分钟没有响应", spec.name, ACP_REQUEST_TIMEOUT.as_secs() / 60);
            }
        };
        let conn = AcpConn {
            cx,
            child: Mutex::new(child),
            pending,
            task: StdMutex::new(Some(task)),
            agent_info: init["agentInfo"].clone(),
            capabilities: init["agentCapabilities"].clone(),
            steering: init["_meta"]["steering"]["supported"] == true,
            _pin: spec.pin.clone(),
        };
        Ok((Arc::new(conn), rx))
    }

    pub async fn request(&self, method: &str, params: Value) -> Result<Value> {
        self.request_timeout(method, params, Some(ACP_REQUEST_TIMEOUT)).await
    }

    /// `None` waits until the agent answers or exits: the caller enforces its own deadline,
    /// e.g. a prompt turn whose clock stops while a person decides on a permission request.
    pub async fn request_timeout(
        &self,
        method: &str,
        params: Value,
        timeout: Option<std::time::Duration>,
    ) -> Result<Value> {
        let sent = self.cx.send_request(UntypedMessage::new(method, params)?);
        let waited = match timeout {
            Some(limit) => tokio::time::timeout(limit, sent.block_task()).await.map_err(|_| limit),
            None => Ok(sent.block_task().await),
        };
        match waited {
            Err(limit) => Err(anyhow!("{method}: ACP 请求超时（{} 秒）", limit.as_secs())),
            Ok(Ok(result)) => Ok(result),
            Ok(Err(err)) => Err(request_error(method, err)),
        }
    }

    pub async fn respond(&self, id: RequestId, result: Value) -> Result<()> {
        if let Some(responder) = self.pending.lock().unwrap().remove(&id) {
            responder.respond(result).map_err(|err| anyhow!("回应 ACP 请求失败：{err}"))?;
        }
        Ok(())
    }

    pub async fn respond_error(&self, id: RequestId, message: &str) -> Result<()> {
        if let Some(responder) = self.pending.lock().unwrap().remove(&id) {
            let mut err = AcpError::method_not_found();
            err.message = message.to_string();
            responder.respond_with_error(err).map_err(|err| anyhow!("回应 ACP 请求失败：{err}"))?;
        }
        Ok(())
    }

    pub async fn notify(&self, method: &str, params: Value) -> Result<()> {
        self.cx.send_notification(UntypedMessage::new(method, params)?)?;
        Ok(())
    }

    pub async fn kill(&self) {
        if let Some(task) = self.task.lock().unwrap().take() {
            task.abort();
        }
        let mut child = self.child.lock().await;
        #[cfg(unix)]
        if let Some(pid) = child.id() {
            let _ = tokio::process::Command::new("kill").args(["-TERM", &format!("-{pid}")]).status().await;
        }
        if tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await.is_err() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
    }
}

impl Drop for AcpConn {
    fn drop(&mut self) {
        if let Ok(slot) = self.task.get_mut()
            && let Some(task) = slot.take()
        {
            task.abort();
        }
    }
}

fn request_error(method: &str, err: AcpError) -> anyhow::Error {
    if is_incoming_transport_closed(&err) || is_connection_terminated(&err) {
        return anyhow!("{method}: Agent 进程已退出");
    }
    let detail = err
        .data
        .as_ref()
        .and_then(|data| data["details"].as_str())
        .or_else(|| (!err.message.is_empty()).then_some(err.message.as_str()))
        .unwrap_or("unknown error");
    anyhow!("{method}: {detail}")
}
