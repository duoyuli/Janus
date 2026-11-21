//! What stays open or comes from the agents: a run's event stream, the app's global stream, and
//! the MCP endpoints of a run's sessions (the shared board and the library router).

use super::runs::After;
use super::{ApiError, ApiResult, reply};
use crate::db;
use crate::orchestrator::App;
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

/// A run's events from `after` on: the backlog, then each new one as it is stored.
pub(super) async fn run(
    State(app): State<Arc<App>>,
    UrlPath(id): UrlPath<String>,
    Query(q): Query<After>,
) -> Sse<impl tokio_stream::Stream<Item = Result<SseEvent, Infallible>>> {
    // Subscribe before reading the backlog so nothing falls between the two.
    let live = BroadcastStream::new(app.tx.subscribe());
    let backlog = app.db.events(&id, q.after.unwrap_or(0)).unwrap_or_default();
    let last = backlog.last().map(|e| e.seq).unwrap_or(q.after.unwrap_or(0));
    let to_sse = |e: &db::Event| Ok(SseEvent::default().data(serde_json::to_string(e).unwrap()));
    let backlog: Vec<_> = backlog.iter().map(to_sse).collect();
    let live = live.filter_map(move |msg| match msg {
        Ok(e) if e.run_id == id && e.seq > last => Some(to_sse(&e)),
        Err(_) => Some(lagged()),
        _ => None,
    });
    Sse::new(tokio_stream::iter(backlog).chain(live)).keep_alive(KeepAlive::default())
}

/// Everything the app follows across runs, as it happens: a short notice for each new event of any
/// run (`event`: run, seq, type), each change of a run's status (`run`: run, status, and `loop` for
/// a run a Loop dispatched) and each change of a workspace Loop (`loop`: loop). The app
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
    let dispatcher = app.clone();
    let changes = BroadcastStream::new(app.changes.subscribe()).map(move |msg| match msg {
        Ok(c) => {
            let mut notice = serde_json::to_value(&c).unwrap_or_default();
            // A run a Loop dispatched names its Loop, whose page counts and lists it.
            if let Some(l) =
                dispatcher.db.run(&c.run).ok().flatten().filter(|r| r["kind"] == "goal").map(|r| r["loopId"].clone())
                && l.is_string()
            {
                notice["loop"] = l;
            }
            Ok(SseEvent::default().event("run").data(notice.to_string()))
        }
        Err(_) => lagged(),
    });
    let loops = BroadcastStream::new(app.loop_changes.subscribe()).map(move |msg| match msg {
        Ok(id) => Ok(SseEvent::default().event("loop").data(json!({ "loop": id }).to_string())),
        Err(_) => lagged(),
    });
    let ready = tokio_stream::once(Ok(SseEvent::default().event("ready").data("{}")));
    Sse::new(ready.chain(events.merge(changes).merge(loops))).keep_alive(KeepAlive::default())
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
/// other local processes out.
pub(super) async fn board(
    State(app): State<Arc<App>>,
    UrlPath((run, token, who)): UrlPath<(String, String, String)>,
    Json(body): Json<Value>,
) -> ApiResult {
    mcp_reply(app.mcp(&run, &token, &who, body).await)
}

/// The run's MCP router (`janus-tools`): the library's servers for everyone.
// @project-doc docs/domains/20261116-resource_library.md#mcp_router
pub(super) async fn tools(
    State(app): State<Arc<App>>,
    UrlPath((run, token, who)): UrlPath<(String, String, String)>,
    Json(body): Json<Value>,
) -> ApiResult {
    mcp_reply(app.mcp_tools(&run, &token, &who, body).await)
}

/// No server-initiated stream: every response comes back on its POST.
pub(super) async fn no_stream() -> StatusCode {
    StatusCode::METHOD_NOT_ALLOWED
}
