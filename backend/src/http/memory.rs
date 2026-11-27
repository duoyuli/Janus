//! A workspace's team memory: its notes and their consolidation, for the app. Agents reach the
//! notes through the `janus` MCP tools, never here.
// @project-doc docs/interfaces/20261031-http_api.md#memories

use super::{ApiError, ApiResult, ok, reply};
use crate::orchestrator::App;
use axum::Json;
use axum::extract::{Path as UrlPath, Query, State};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Deserialize)]
pub(super) struct MemoriesQuery {
    /// `zh` or `en`, for what the service writes about the notes; the app's language without.
    lang: Option<String>,
}

/// A workspace's notes, with the consolidation running now, the last one and the runs since.
pub(super) async fn list(
    State(app): State<Arc<App>>,
    UrlPath(workspace): UrlPath<String>,
    Query(q): Query<MemoriesQuery>,
) -> ApiResult {
    let lang = super::views::lang_of(&app, q.lang.as_deref())?;
    reply(app.memories(&workspace, lang)?)
}

/// Consolidate a workspace's notes now; the consolidation's run comes back.
pub(super) async fn consolidate(State(app): State<Arc<App>>, UrlPath(workspace): UrlPath<String>) -> ApiResult {
    reply(json!({ "run": app.consolidate(&workspace).await? }))
}

/// The person rewrites a note: any of `kind`, `title`, `hook` and `body`.
pub(super) async fn update(
    State(app): State<Arc<App>>,
    UrlPath(id): UrlPath<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    match app.edit_memory(&id, &body)? {
        Some(note) => reply(note),
        None => Err(ApiError::not_found(format!("笔记 {id} 不存在"))),
    }
}

pub(super) async fn remove(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> ApiResult {
    if !app.delete_memory(&id)? {
        return Err(ApiError::not_found(format!("笔记 {id} 不存在")));
    }
    ok()
}
