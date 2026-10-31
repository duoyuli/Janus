//! Minimal ACP client: JSON-RPC 2.0 over the agent's stdio, one JSON object per line.
//! Janus is the ACP client; Lody's adapters (acp-extension-codex / acp-extension-pi) are the agents.

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{Mutex, mpsc, oneshot};

const ACP_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30 * 60);

#[derive(Clone, Debug)]
pub struct RuntimeSpec {
    pub id: String,
    /// codex / claude / pi: decides adapter-specific session settings.
    pub kind: String,
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
    /// The complete environment. Nothing is inherited from Janus's own process: an agent sees
    /// only these variables, so the user's shell setup, credentials and agent settings cannot leak in.
    pub env: Vec<(String, String)>,
}

pub enum Incoming {
    Notification {
        method: String,
        params: Value,
    },
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    Closed,
}

type Pending = Arc<StdMutex<HashMap<u64, oneshot::Sender<Result<Value, Value>>>>>;

pub struct AcpConn {
    stdin: Mutex<ChildStdin>,
    child: Mutex<Child>,
    next: AtomicU64,
    pending: Pending,
    pub agent_info: Value,
    pub capabilities: Value,
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
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(stderr_log)?;
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

        let reader_pending = pending.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                let method = msg.get("method").and_then(Value::as_str).map(str::to_owned);
                match (msg.get("id"), method) {
                    (Some(id), Some(method)) => {
                        let _ = tx.send(Incoming::Request {
                            id: id.clone(),
                            method,
                            params: msg["params"].clone(),
                        });
                    }
                    (Some(id), None) => {
                        let waiter = id
                            .as_u64()
                            .and_then(|id| reader_pending.lock().unwrap().remove(&id));
                        if let Some(waiter) = waiter {
                            let _ = waiter.send(match msg.get("error") {
                                Some(err) => Err(err.clone()),
                                None => Ok(msg["result"].clone()),
                            });
                        }
                    }
                    (None, Some(method)) => {
                        let _ = tx.send(Incoming::Notification {
                            method,
                            params: msg["params"].clone(),
                        });
                    }
                    _ => {}
                }
            }
            reader_pending.lock().unwrap().clear();
            let _ = tx.send(Incoming::Closed);
        });

        let mut conn = AcpConn {
            stdin: Mutex::new(stdin),
            child: Mutex::new(child),
            next: AtomicU64::new(1),
            pending,
            agent_info: Value::Null,
            capabilities: Value::Null,
        };
        let init = conn
            .request(
                "initialize",
                json!({
                    "protocolVersion": 1,
                    // Forms and links an agent asks the person to fill in or open (AskUserQuestion, MCP input,
                    // plan feedback) are rendered in the inbox next to permission requests.
                    "clientCapabilities": { "fs": { "readTextFile": false, "writeTextFile": false }, "terminal": false, "elicitation": { "form": {}, "url": {} } },
                    "clientInfo": { "name": "janus", "version": env!("CARGO_PKG_VERSION") }
                }),
            )
            .await
            .with_context(|| format!("{} 的 ACP initialize 失败", spec.name))?;
        conn.agent_info = init["agentInfo"].clone();
        conn.capabilities = init["agentCapabilities"].clone();
        Ok((Arc::new(conn), rx))
    }

    pub async fn request(&self, method: &str, params: Value) -> Result<Value> {
        self.request_timeout(method, params, Some(ACP_REQUEST_TIMEOUT))
            .await
    }

    /// `None` waits until the agent answers or exits: the caller enforces its own deadline,
    /// e.g. a prompt turn whose clock stops while a person decides on a permission request.
    pub async fn request_timeout(
        &self,
        method: &str,
        params: Value,
        timeout: Option<std::time::Duration>,
    ) -> Result<Value> {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);
        if let Err(err) = self
            .write(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
            .await
        {
            self.pending.lock().unwrap().remove(&id);
            return Err(err);
        }
        let waited = match timeout {
            Some(limit) => tokio::time::timeout(limit, rx).await.map_err(|_| limit),
            None => Ok(rx.await),
        };
        match waited {
            Err(limit) => {
                self.pending.lock().unwrap().remove(&id);
                Err(anyhow!("{method}: ACP 请求超时（{} 秒）", limit.as_secs()))
            }
            Ok(result) => match result {
                Ok(Ok(result)) => Ok(result),
                Ok(Err(err)) => {
                    let detail = err["data"]["details"]
                        .as_str()
                        .or(err["message"].as_str())
                        .unwrap_or("unknown error");
                    bail!("{method}: {detail}")
                }
                Err(_) => Err(anyhow!("{method}: Agent 进程已退出")),
            },
        }
    }

    pub async fn respond(&self, id: Value, result: Value) -> Result<()> {
        self.write(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
            .await
    }

    pub async fn respond_error(&self, id: Value, message: &str) -> Result<()> {
        self.write(
            json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": message } }),
        )
        .await
    }

    async fn write(&self, msg: Value) -> Result<()> {
        let mut line = serde_json::to_vec(&msg)?;
        line.push(b'\n');
        let mut stdin = self.stdin.lock().await;
        stdin.write_all(&line).await?;
        stdin.flush().await?;
        Ok(())
    }

    pub async fn notify(&self, method: &str, params: Value) -> Result<()> {
        self.write(json!({ "jsonrpc": "2.0", "method": method, "params": params }))
            .await
    }

    pub async fn kill(&self) {
        let mut child = self.child.lock().await;
        #[cfg(unix)]
        if let Some(pid) = child.id() {
            let _ = tokio::process::Command::new("kill")
                .args(["-TERM", &format!("-{pid}")])
                .status()
                .await;
        }
        if tokio::time::timeout(std::time::Duration::from_secs(5), child.wait())
            .await
            .is_err()
        {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
    }
}
