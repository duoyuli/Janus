//! What a run's log comes to and what waits on the person, as `run_view` works them out: the app
//! and any other client draw these and compute none of it.

use super::{ApiError, ApiResult, ok, reply};
use crate::db::InboxMark;
use crate::lang::Lang;
use crate::orchestrator::App;
use axum::Json;
use axum::extract::{Path as UrlPath, Query, State};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

#[derive(Deserialize)]
pub(super) struct ViewQuery {
    /// Up to and including this seq (a replayed position); the whole log without.
    until: Option<i64>,
    /// `zh` or `en`; the app's language without.
    lang: Option<String>,
}

/// The language a request asks for, or the app's.
fn lang_of(app: &App, lang: Option<&str>) -> Result<Lang, ApiError> {
    match lang {
        None => Ok(app.language()),
        Some(l) => Lang::parse(l).ok_or_else(|| ApiError::bad_request(format!("未知的语言：{l}"))),
    }
}

/// Run `id`'s tasks, agents, requests, messages, deliveries, board and review at one position.
// @project-doc docs/interfaces/20261031-http_api.md#run_view
pub(super) async fn run(
    State(app): State<Arc<App>>,
    UrlPath(id): UrlPath<String>,
    Query(q): Query<ViewQuery>,
) -> ApiResult {
    let lang = lang_of(&app, q.lang.as_deref())?;
    match app.run_view(&id, q.until, lang)? {
        Some(view) => reply(json!(view)),
        None => Err(ApiError::not_found("任务不存在")),
    }
}

#[derive(Deserialize)]
pub(super) struct InboxQuery {
    lang: Option<String>,
}

/// Everything that waits on the person, across runs.
// @project-doc docs/interfaces/20261031-http_api.md#inbox
pub(super) async fn inbox(State(app): State<Arc<App>>, Query(q): Query<InboxQuery>) -> ApiResult {
    let lang = lang_of(&app, q.lang.as_deref())?;
    reply(json!(app.inbox(lang, crate::util::now_ms())?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MarkBody {
    key: String,
    /// Put off until then (ms); none brings it back.
    snoozed_until: Option<i64>,
    #[serde(default)]
    ignored: bool,
}

/// Put an inbox entry off, bring it back, or stop being reminded of it.
// @project-doc docs/interfaces/20261031-http_api.md#inbox
pub(super) async fn mark(State(app): State<Arc<App>>, Json(b): Json<MarkBody>) -> ApiResult {
    if b.key.is_empty() {
        return Err(ApiError::bad_request("缺少 key"));
    }
    app.mark_inbox(&b.key, InboxMark { snoozed_until: b.snoozed_until, ignored: b.ignored })?;
    ok()
}
