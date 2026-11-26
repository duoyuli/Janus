//! What stays open or comes from the agents: a run's event stream, the app's global stream, and
//! the MCP endpoints of a run's sessions (the shared board and the library router).

use super::runs::After;
use super::{ApiError, ApiResult, reply};
use crate::db;
use crate::orchestrator::{App, Change, SessionKey};
use axum::Json;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use serde_json::{Value, json};
use std::convert::Infallible;
use std::sync::Arc;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;

/// Tells the app it missed events and should read again rather than silently skip them.
fn lagged() -> Result<SseEvent, Infallible> {
    Ok(SseEvent::default().event("lagged").data("{}"))
}

/// A run's events from `after` on: the backlog, then each new one as it is stored. Beside them,
/// `live` frames preview what its sessions are writing and running before it is on record; a
/// frame missed is not worth a reload, so they never ask for one.
// @project-doc docs/interfaces/20261031-acp_integration.md#live_frames
pub(super) async fn run(
    State(app): State<Arc<App>>,
    UrlPath(id): UrlPath<String>,
    Query(q): Query<After>,
) -> Sse<impl tokio_stream::Stream<Item = Result<SseEvent, Infallible>>> {
    // Subscribe before reading the backlog so nothing falls between the two.
    let live = BroadcastStream::new(app.tx.subscribe());
    let frames = BroadcastStream::new(app.live.subscribe());
    let backlog = app.db.events(&id, q.after.unwrap_or(0)).unwrap_or_default();
    let last = backlog.last().map(|e| e.seq).unwrap_or(q.after.unwrap_or(0));
    let to_sse = |e: &db::Event| Ok(SseEvent::default().data(serde_json::to_string(e).unwrap()));
    let backlog: Vec<_> = backlog.iter().map(to_sse).collect();
    let run_id = id.clone();
    let live = live.filter_map(move |msg| match msg {
        Ok(e) if e.run_id == id && e.seq > last => Some(to_sse(&e)),
        Err(_) => Some(lagged()),
        _ => None,
    });
    let frames = frames.filter_map(move |msg| match msg {
        Ok(f) if f.run == run_id => Some(Ok(SseEvent::default().event("live").data(f.data.to_string()))),
        _ => None,
    });
    Sse::new(tokio_stream::iter(backlog).chain(live.merge(frames))).keep_alive(KeepAlive::default())
}

/// Everything the app follows across runs, as it happens: a short notice for each new event of any
/// run (`event`: run, seq, type), each change of a run's status (`run`: run, status, and `loop` for
/// a run a Loop dispatched), each change of a workspace Loop (`loop`: loop) and of a workspace's
/// team memory (`memory`: workspace). The app
/// reads what it needs about a run only when that run has news, instead of polling every run.
/// `ready` opens the stream (the app reads its lists once then), `lagged` asks it to read again.
// @project-doc docs/interfaces/20261031-http_api.md#global_stream
pub(super) async fn all(
    State(app): State<Arc<App>>,
) -> Sse<impl tokio_stream::Stream<Item = Result<SseEvent, Infallible>>> {
    let events = BroadcastStream::new(app.tx.subscribe()).map(move |msg| match msg {
        Ok(e) => Ok(SseEvent::default()
            .event("event")
            .data(json!({ "run": e.run_id, "seq": e.seq, "type": e.kind }).to_string())),
        Err(_) => lagged(),
    });
    let changes = BroadcastStream::new(app.changes.subscribe()).map(move |msg| match msg {
        Ok(Change::Run { run, status, loop_id }) => {
            let mut notice = json!({ "run": run, "status": status });
            // A run a Loop dispatched names its Loop, whose page counts and lists it.
            if let Some(l) = loop_id {
                notice["loop"] = json!(l);
            }
            Ok(SseEvent::default().event("run").data(notice.to_string()))
        }
        Ok(Change::Loop(id)) => Ok(SseEvent::default().event("loop").data(json!({ "loop": id }).to_string())),
        Ok(Change::Memory { workspace }) => {
            Ok(SseEvent::default().event("memory").data(json!({ "workspace": workspace }).to_string()))
        }
        Ok(Change::Library) => Ok(SseEvent::default().event("library").data("{}")),
        Ok(Change::Connection(id)) => {
            Ok(SseEvent::default().event("connection").data(json!({ "connection": id }).to_string()))
        }
        Ok(Change::Adapters) => Ok(SseEvent::default().event("adapters").data("{}")),
        Ok(Change::Inbox) => Ok(SseEvent::default().event("inbox").data("{}")),
        Err(_) => lagged(),
    });
    let ready = tokio_stream::once(Ok(SseEvent::default().event("ready").data("{}")));
    Sse::new(ready.chain(events.merge(changes))).keep_alive(KeepAlive::default())
}

/// The answer to one MCP message: its response, or 202 for a notification. A request whose
/// session is not let in (another run, a wrong token) is refused with 403.
fn mcp_reply(answer: anyhow::Result<Option<Value>>) -> ApiResult {
    match answer {
        Ok(Some(response)) => reply(response),
        Ok(None) => Ok(StatusCode::ACCEPTED.into_response()),
        Err(err) => Err(ApiError::forbidden(format!("{err:#}"))),
    }
}

/// The shared board as an MCP server (streamable HTTP, JSON responses) for the sessions of one
/// run: the lead's orchestration tools, or a collaborator's. The token keeps other runs and
/// other local processes out; the path says whose session it is (`SessionKey`).
pub(super) async fn lead_board(
    State(app): State<Arc<App>>,
    UrlPath((run, token)): UrlPath<(String, String)>,
    Json(body): Json<Value>,
) -> ApiResult {
    mcp_reply(app.mcp(&run, &token, &SessionKey::Lead, body).await)
}

pub(super) async fn task_board(
    State(app): State<Arc<App>>,
    UrlPath((run, token, task)): UrlPath<(String, String, String)>,
    Json(body): Json<Value>,
) -> ApiResult {
    mcp_reply(app.mcp(&run, &token, &SessionKey::Task(task), body).await)
}

/// The run's MCP router (`janus-tools`): the library's servers for everyone.
// @project-doc docs/domains/20261116-resource_library.md#mcp_router
pub(super) async fn lead_tools(
    State(app): State<Arc<App>>,
    UrlPath((run, token)): UrlPath<(String, String)>,
    Json(body): Json<Value>,
) -> ApiResult {
    mcp_reply(app.mcp_tools(&run, &token, &SessionKey::Lead, body).await)
}

pub(super) async fn task_tools(
    State(app): State<Arc<App>>,
    UrlPath((run, token, task)): UrlPath<(String, String, String)>,
    Json(body): Json<Value>,
) -> ApiResult {
    mcp_reply(app.mcp_tools(&run, &token, &SessionKey::Task(task), body).await)
}

/// No server-initiated stream: every response comes back on its POST.
pub(super) async fn no_stream() -> StatusCode {
    StatusCode::METHOD_NOT_ALLOWED
}
