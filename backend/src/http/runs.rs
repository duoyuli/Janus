//! Runs and chats: listing, creating, acting on and deleting them, their usage and files, and the
//! usage bill and model prices across runs.

use super::setup::PathQuery;
use super::{ApiError, ApiResult, ok, reply};
use crate::orchestrator::{ActionReply, ActionRequest, App, NewChat, NewRun};
use crate::status::{RunKind, RunStatus};
use crate::{acp_view, bill, db, prices, usage};
use axum::Json;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::header;
use axum::response::IntoResponse;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Deserialize)]
pub(super) struct RunsQuery {
    kind: Option<String>,
    workspace: Option<String>,
    /// `1`: only runs that may need the person (working, delivered, stopped part-way), however old.
    attention: Option<u8>,
    /// Only the runs this Loop dispatched.
    r#loop: Option<String>,
    /// `next` of the previous page.
    cursor: Option<String>,
    limit: Option<usize>,
}

/// Runs of one kind, newest first, a page at a time: goals (the default), chats (`?kind=chat`) or
/// Loop sessions (`?kind=loop`), of one workspace with `?workspace=`, dispatched by one Loop with `?loop=`. A page has `limit` runs (50 by default, at most 500) and
/// `next`, the cursor of the page after it (`null` on the last page).
// @project-doc docs/interfaces/20261031-http_api.md#run_list
pub(super) async fn list(State(app): State<Arc<App>>, Query(q): Query<RunsQuery>) -> ApiResult {
    let kind = match q.kind.as_deref() {
        None => RunKind::Goal,
        Some(k) => RunKind::parse(k).ok_or_else(|| ApiError::bad_request(format!("未知的类型：{k}")))?,
    };
    let before = match q.cursor.as_deref() {
        None => None,
        Some(c) => Some(
            c.split_once(':')
                .and_then(|(t, id)| Some((t.parse::<i64>().ok()?, id.to_string())))
                .ok_or_else(|| ApiError::bad_request(format!("cursor 不对：{c}")))?,
        ),
    };
    use RunStatus::*;
    let statuses = (q.attention == Some(1)).then(|| vec![Planning, Running, Delivered, Interrupted, Failed]);
    let page = db::RunsPage {
        kind,
        workspace: q.workspace,
        statuses,
        loop_id: q.r#loop,
        before,
        limit: q.limit.unwrap_or(50).clamp(1, 500),
    };
    let (runs, more) = app.db.runs_page(&page)?;
    let next = runs.last().filter(|_| more).map(|r| format!("{}:{}", r.created_at, r.id.as_str()));
    reply(json!({ "runs": runs, "next": next }))
}

/// Every workspace that goal runs were made in, as their snapshots name it, deleted ones included.
// @project-doc docs/interfaces/20261031-http_api.md#run_list
pub(super) async fn workspaces(State(app): State<Arc<App>>) -> ApiResult {
    reply(json!({ "workspaces": app.db.run_workspaces()? }))
}

/// Start a conversation with one digital employee; it opens with the first message.
// @project-doc docs/interfaces/20261031-http_api.md#chats
pub(super) async fn create_chat(State(app): State<Arc<App>>, Json(req): Json<NewChat>) -> ApiResult {
    reply(json!({ "id": app.create_chat(req).await? }))
}

#[derive(Deserialize)]
pub(super) struct PreflightQuery {
    workspace: String,
}

/// What to remind the person of before a run starts in a workspace (they may go ahead anyway).
// @project-doc docs/interfaces/20261031-http_api.md#run_preflight
pub(super) async fn preflight(State(app): State<Arc<App>>, Query(q): Query<PreflightQuery>) -> ApiResult {
    reply(json!({ "warnings": app.preflight(&q.workspace).await? }))
}

pub(super) async fn create(State(app): State<Arc<App>>, Json(req): Json<NewRun>) -> ApiResult {
    reply(json!({ "id": app.create_run(req).await? }))
}

/// Remove a run with every record and file Janus kept for it. Only a run that is not working can
/// go; the workspace's own files are not touched.
// @project-doc docs/interfaces/20261031-http_api.md#run_delete
pub(super) async fn remove(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> ApiResult {
    app.delete_run_or_loop(&id).await?;
    ok()
}

#[derive(Deserialize)]
pub(super) struct After {
    pub(super) after: Option<i64>,
}

/// The stored record of run `id`, or 404.
fn stored_run(app: &App, id: &str) -> Result<db::RunRow, ApiError> {
    app.db.run(id)?.ok_or_else(|| ApiError::not_found("任务不存在"))
}

/// One run and its events; with `after`, only the events past that seq, so a client that already
/// holds the rest (the app's inbox) reads just what is new.
pub(super) async fn get(
    State(app): State<Arc<App>>,
    UrlPath(id): UrlPath<String>,
    Query(q): Query<After>,
) -> ApiResult {
    let run = stored_run(&app, &id)?;
    let events = app.db.events(&id, q.after.unwrap_or(0).max(0))?;
    reply(json!({ "run": run, "events": events, "live": app.run(&id).is_some() }))
}

#[derive(Deserialize)]
pub(super) struct Until {
    until: Option<i64>,
}

/// The run's read-only usage ledger over its event log up to and including seq `until` (all of it
/// without), from `backend/src/usage.rs`: the app reads it for the live run and for each replayed
/// position. A run always has its `run.created` event, so no events means no such run.
// @project-doc docs/interfaces/20261031-http_api.md#run_usage
pub(super) async fn usage(
    State(app): State<Arc<App>>,
    UrlPath(id): UrlPath<String>,
    Query(q): Query<Until>,
) -> ApiResult {
    let mut events = app.db.events(&id, 0)?;
    if events.is_empty() {
        return Err(ApiError::not_found("任务不存在"));
    }
    if let Some(until) = q.until {
        events.retain(|e| e.seq <= until);
    }
    reply(usage::ledger(&events))
}

#[derive(Deserialize)]
pub(super) struct Period {
    from: i64,
    to: i64,
    /// The person's time zone in minutes east of UTC, so active days are their days.
    offset: Option<i64>,
}

/// The usage bill over a period, across runs (`backend/src/bill.rs`).
pub(super) async fn usage_bill(State(app): State<Arc<App>>, Query(p): Query<Period>) -> ApiResult {
    const MAX_DAYS: i64 = 400;
    if p.to <= p.from || p.to - p.from > MAX_DAYS * 24 * 60 * 60 * 1000 {
        return Err(ApiError::bad_request(format!("区间要在 1 毫秒到 {MAX_DAYS} 天之间")));
    }
    let prices = app.prices();
    reply(bill::period(&app.db, p.from, p.to, p.offset.unwrap_or(0), prices.as_deref())?)
}

#[derive(Deserialize)]
pub(super) struct PriceQuery {
    /// Model names, comma separated, as the sessions name them.
    models: Option<String>,
}

/// List prices of the given models from the synced models.dev list (`backend/src/prices.rs`), so
/// the app can price a task's bill; `null` for a model without a price.
// @project-doc docs/interfaces/20261031-http_api.md#prices
pub(super) async fn prices(State(app): State<Arc<App>>, Query(q): Query<PriceQuery>) -> ApiResult {
    reply(price_reply(app.prices().as_deref(), q.models.as_deref().unwrap_or("")))
}

/// Download the models.dev list now instead of waiting for the daily sync.
pub(super) async fn sync_prices(State(app): State<Arc<App>>) -> ApiResult {
    let table = app.sync_prices(true).await?;
    reply(price_reply(Some(&table), ""))
}

fn price_reply(table: Option<&prices::Table>, models: &str) -> Value {
    let found: serde_json::Map<String, Value> = models
        .split(',')
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .map(|m| (m.to_string(), json!(table.and_then(|t| t.get(m)))))
        .collect();
    json!({ "source": prices::SOURCE, "fetchedAt": table.map(|t| t.fetched_at), "count": table.map_or(0, |t| t.models.len()), "prices": found })
}

#[derive(Deserialize)]
pub(super) struct FileQuery {
    q: Option<String>,
}

/// Files in a run's workspace whose path contains `q`, for the `@` menu of the composer.
// @project-doc docs/interfaces/20261031-http_api.md#run_files
pub(super) async fn files(
    State(app): State<Arc<App>>,
    UrlPath(id): UrlPath<String>,
    Query(q): Query<FileQuery>,
) -> ApiResult {
    let run = stored_run(&app, &id)?;
    let cwd = PathBuf::from(&run.cwd);
    reply(json!({ "files": acp_view::workspace_files(&cwd, q.q.as_deref().unwrap_or(""), 20) }))
}

/// The digital employees a chat proposed and the person has not acted on yet (`null`: none),
/// numbered and marked with a saved one of the same name.
// @project-doc docs/interfaces/20261031-http_api.md#proposed_employees
pub(super) async fn proposal(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> ApiResult {
    stored_run(&app, &id)?;
    reply(app.open_proposal(&id)?)
}

#[derive(Deserialize)]
pub(super) struct Proposed {
    proposal: i64,
    #[serde(default)]
    picks: Vec<String>,
    #[serde(default)]
    connection: String,
}

/// Create the picked employees (`picks`, their numbers) of the chat's open proposal on one agent.
// @project-doc docs/interfaces/20261031-http_api.md#proposed_employees
pub(super) async fn create_proposed(
    State(app): State<Arc<App>>,
    UrlPath(id): UrlPath<String>,
    Json(p): Json<Proposed>,
) -> ApiResult {
    stored_run(&app, &id)?;
    reply(app.create_proposed(&id, p.proposal, &p.picks, &p.connection)?)
}

/// Create none of the chat's open proposal.
pub(super) async fn decline_proposed(
    State(app): State<Arc<App>>,
    UrlPath(id): UrlPath<String>,
    Json(p): Json<Proposed>,
) -> ApiResult {
    stored_run(&app, &id)?;
    app.decline_proposed(&id, p.proposal)?;
    ok()
}

pub(super) async fn action(
    State(app): State<Arc<App>>,
    UrlPath(id): UrlPath<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let req: ActionRequest =
        serde_json::from_value(body).map_err(|e| ApiError::bad_request(format!("操作的内容不对：{e}")))?;
    match app.request_action(&id, &req).await? {
        ActionReply::Done => ok(),
        ActionReply::Duplicate => reply(json!({ "ok": true, "duplicate": true, "commandId": req.command_id })),
    }
}

pub(super) async fn export(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> ApiResult {
    let lines: Vec<String> = app.db.events(&id, 0)?.iter().map(|e| serde_json::to_string(e).unwrap()).collect();
    Ok((
        [
            (header::CONTENT_TYPE, "application/x-ndjson; charset=utf-8"),
            (header::CONTENT_DISPOSITION, &format!("attachment; filename=\"janus-{id}.jsonl\"")),
        ],
        lines.join("\n") + "\n",
    )
        .into_response())
}

/// Artifacts are served read-only, and nothing else: the rest of Janus home holds the database
/// and credentials.
pub(super) async fn file(State(app): State<Arc<App>>, Query(q): Query<PathQuery>) -> ApiResult {
    let path = PathBuf::from(&q.path);
    if !path.exists() {
        return Err(ApiError::not_found("文件不存在"));
    }
    if !app.cfg.layout.is_artifact(&path) {
        return Err(ApiError::forbidden("只能读取 Janus 的产物"));
    }
    let text = std::fs::read_to_string(&path).map_err(anyhow::Error::from)?;
    Ok(([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], text).into_response())
}
