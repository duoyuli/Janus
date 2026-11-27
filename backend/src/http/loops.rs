//! Workspace Loops: listing, making, changing, waking, pausing and deleting them. A Loop's session
//! is a run of kind `loop`, read and acted on like any run (`/api/runs/{id}`).
// @project-doc docs/interfaces/20261031-http_api.md#loops

use super::{ApiResult, ok, reply};
use crate::orchestrator::{App, WakeReason, loop_words};
use crate::schedule::Schedule;
use axum::Json;
use axum::extract::{Path as UrlPath, Query, State};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Deserialize)]
pub(super) struct LoopsQuery {
    workspace: Option<String>,
    /// `zh` or `en`, for what the service writes about each Loop; the app's language without.
    lang: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct LangQuery {
    lang: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct ScheduleQuery {
    cron: String,
    tz: Option<String>,
    lang: Option<String>,
}

/// A schedule checked as a Loop would take it (`?cron=&tz=`), with the next three times it comes
/// round, its shape and its words, so the clients show them without reading cron themselves; 400
/// with the reason when it is wrong.
pub(super) async fn schedule(State(app): State<Arc<App>>, Query(q): Query<ScheduleQuery>) -> ApiResult {
    let lang = super::views::lang_of(&app, q.lang.as_deref())?;
    let s = Schedule::new(&q.cron, q.tz.as_deref())?;
    let next = s.upcoming(crate::util::now_ms(), 3);
    let shape = loop_words::shape(&s.cron);
    let words = loop_words::words(&shape, lang);
    reply(json!({ "cron": s.cron, "tz": s.tz, "next": next, "shape": shape, "words": words }))
}

/// Every Loop (of one workspace with `?workspace=`), with what each is doing now and what the
/// clients show of it (`loop_words::shown`).
pub(super) async fn list(State(app): State<Arc<App>>, Query(q): Query<LoopsQuery>) -> ApiResult {
    let lang = super::views::lang_of(&app, q.lang.as_deref())?;
    let loops: Vec<Value> =
        app.loops(q.workspace.as_deref())?.into_iter().map(|l| loop_words::shown(l, lang)).collect();
    reply(json!({ "loops": loops }))
}

/// A new Loop in a workspace, run by its lead: `prompt`, `schedule` (`{cron, tz}`, or null for
/// only when called) and `wakeNow`.
pub(super) async fn create(
    State(app): State<Arc<App>>,
    UrlPath(workspace): UrlPath<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    reply(app.create_loop(&workspace, &body).await?)
}

pub(super) async fn get(
    State(app): State<Arc<App>>,
    UrlPath(id): UrlPath<String>,
    Query(q): Query<LangQuery>,
) -> ApiResult {
    let lang = super::views::lang_of(&app, q.lang.as_deref())?;
    reply(loop_words::shown(app.loop_view(&app.loop_def(&id)?)?, lang))
}

/// Change a Loop's `name`, `prompt` and `schedule`.
pub(super) async fn update(
    State(app): State<Arc<App>>,
    UrlPath(id): UrlPath<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    reply(app.update_loop(&id, &body)?)
}

/// Delete a Loop and its session; the runs it dispatched stay.
pub(super) async fn remove(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> ApiResult {
    app.delete_loop(&id).await?;
    ok()
}

/// Wake a Loop now (`wake`), or stop and start its schedule (`pause`, `resume`).
pub(super) async fn act(State(app): State<Arc<App>>, UrlPath((id, action)): UrlPath<(String, String)>) -> ApiResult {
    match action.as_str() {
        "wake" => {
            app.loop_def(&id)?;
            app.wake_loop_later(&id, WakeReason::Manual);
            ok()
        }
        "pause" => reply(app.pause_loop(&id, true)?),
        "resume" => reply(app.pause_loop(&id, false)?),
        _ => Err(super::ApiError::not_found(format!("没有这个操作：{action}"))),
    }
}
