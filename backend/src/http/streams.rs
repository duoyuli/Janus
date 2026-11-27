//! What stays open or comes from the agents: a run's event stream, the app's global stream, and
//! the MCP endpoints of a run's sessions (the shared board and the library router).

use super::runs::After;
use super::{ApiError, ApiResult, reply};
use crate::db;
use crate::lang::Lang;
use crate::orchestrator::{App, Change, SessionKey};
use crate::run_view::preview::Previews;
use axum::Json;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use serde::Deserialize;
use serde_json::{Value, json};
use std::convert::Infallible;
use std::sync::Arc;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};

/// Tells the app it missed events and should read again rather than silently skip them.
fn lagged() -> Result<SseEvent, Infallible> {
    Ok(SseEvent::default().event("lagged").data("{}"))
}

/// A run's events from `after` on: the backlog, then each new one as it is stored. Beside them,
/// `live` frames preview what its conversations are writing and running before it is on record
/// (`run_view::preview`): each frame holds the whole preview of the conversations it changed, or
/// null for one with nothing left to preview. A frame missed is not worth a reload, so they never
/// ask for one.
// @project-doc docs/interfaces/20261031-acp_integration.md#live_frames
pub(super) async fn run(
    State(app): State<Arc<App>>,
    UrlPath(id): UrlPath<String>,
    Query(q): Query<After>,
) -> Sse<impl tokio_stream::Stream<Item = Result<SseEvent, Infallible>>> {
    let lang = super::views::lang_of(&app, q.lang.as_deref()).unwrap_or_else(|_| app.language());
    Sse::new(run_events(&app, id, q.after.unwrap_or(0), lang)).keep_alive(KeepAlive::default())
}

/// Run `id`'s events after `after` and its `live` frames, as the stream sends them (unnamed
/// events and `live`), until the client goes: what a run's own stream and the global stream
/// carrying one run (`/api/stream?run=`) both send.
fn run_events(app: &Arc<App>, id: String, after: i64, lang: Lang) -> ReceiverStream<Result<SseEvent, Infallible>> {
    // Subscribe before reading the backlog so nothing falls between the two.
    let mut events = app.tx.subscribe();
    let mut frames = app.live.subscribe();
    let cwd = app.db.run(&id).ok().flatten().map(|r| r.cwd).unwrap_or_default();
    let backlog = app.db.events(&id, after).unwrap_or_default();
    let last = backlog.last().map(|e| e.seq).unwrap_or(after);
    let (out, rx) = tokio::sync::mpsc::channel::<Result<SseEvent, Infallible>>(256);
    tokio::spawn(async move {
        use tokio::sync::broadcast::error::RecvError;
        let to_sse = |e: &db::Event| Ok(SseEvent::default().data(serde_json::to_string(e).unwrap()));
        let live = |f: Value| Ok(SseEvent::default().event("live").data(f.to_string()));
        let mut previews = Previews::new(lang, cwd);
        for e in &backlog {
            previews.recorded(e);
            if out.send(to_sse(e)).await.is_err() {
                return;
            }
        }
        loop {
            let sent = tokio::select! {
                _ = out.closed() => return,
                msg = events.recv() => match msg {
                    Ok(e) if e.run_id == id && e.seq > last => {
                        let keys = previews.recorded(&e);
                        let ok = out.send(to_sse(&e)).await.is_ok();
                        match previews.frame(&keys) {
                            Some(f) if ok => out.send(live(f)).await.is_ok(),
                            _ => ok,
                        }
                    }
                    Ok(_) => true,
                    Err(RecvError::Lagged(_)) => out.send(lagged()).await.is_ok(),
                    Err(RecvError::Closed) => return,
                },
                msg = frames.recv() => match msg {
                    Ok(f) if f.run == id => {
                        let keys = previews.apply(&f.data);
                        match previews.frame(&keys) {
                            Some(f) => out.send(live(f)).await.is_ok(),
                            None => true,
                        }
                    }
                    Err(RecvError::Closed) => return,
                    _ => true,
                },
            };
            if !sent {
                return;
            }
        }
    });
    ReceiverStream::new(rx)
}

/// What the global stream also carries: run `run`'s own events from `after` on, as its own stream
/// sends them. A browser holds at most six connections to one host, so a web page keeps a single
/// stream open (a run page as well as the lists), not one per run on top of the global one.
#[derive(Deserialize)]
pub(super) struct AllQuery {
    run: Option<String>,
    after: Option<i64>,
    lang: Option<String>,
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
    Query(q): Query<AllQuery>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<SseEvent, Infallible>>>, ApiError> {
    let one_run = match q.run {
        Some(id) => {
            let lang = super::views::lang_of(&app, q.lang.as_deref())?;
            Some(run_events(&app, id, q.after.unwrap_or(0), lang))
        }
        None => None,
    };
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
        Ok(Change::Devices) => Ok(SseEvent::default().event("devices").data("{}")),
        Err(_) => lagged(),
    });
    let ready = tokio_stream::once(Ok(SseEvent::default().event("ready").data("{}")));
    // Without a run, a stream that ends at once: its sender is dropped here.
    let one_run = one_run.unwrap_or_else(|| ReceiverStream::new(tokio::sync::mpsc::channel(1).1));
    Ok(Sse::new(ready.chain(events.merge(changes).merge(one_run))).keep_alive(KeepAlive::default()))
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
