mod acp;
mod acp_view;
mod adapters;
mod agents;
mod attachments;
mod bill;
mod config;
mod data_shape;
mod db;
mod evidence;
mod git_probe;
mod goal_settle;
mod layout;
mod library;
mod migrations;
mod orchestrator;
mod prices;
mod status;
mod teamwork;
mod traces;
mod usage;
mod worktrees;

use anyhow::Result;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::{StatusCode, header};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use orchestrator::{ActionRefused, ActionReply, App, Config, DeleteRefused, NewChat, NewRun, expand_home, git};
use serde::Deserialize;
use serde_json::{Value, json};
use status::RunKind;
use std::collections::HashMap;
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;

/// Janus home (~/.janus): every user-level thing Janus keeps. Owner-only,
/// since it holds credentials.
fn janus_home() -> Result<PathBuf> {
    let dir = config::settings().home.clone();
    if dir.as_os_str().is_empty() {
        anyhow::bail!("找不到用户目录：请设置 HOME 或 JANUS_HOME");
    }
    std::fs::create_dir_all(&dir)?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    Ok(dir)
}

/// One Janus per home: a second one would mark the first one's running runs as interrupted on
/// startup (`recover`) and drive the same database. The lock is an exclusive OS lock on
/// `<home>/janus.pid`, taken atomically and released when the process ends however it ends; the
/// file also names the owner's pid, which the app reads to find a running Janus. Keep the
/// returned file for as long as the process runs.
// @project-doc docs/operations/20261031-deployment.md#single_instance
fn lock_home(layout: &layout::Layout) -> Result<std::fs::File> {
    use std::io::Write;
    let pid = layout.pid_file();
    let mut file = std::fs::OpenOptions::new().create(true).truncate(false).read(true).write(true).open(&pid)?;
    match file.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => {
            let owner = std::fs::read_to_string(&pid).unwrap_or_default();
            anyhow::bail!(
                "另一个 Janus（pid {}）正在使用 {}。先停止它（以服务方式运行时执行 just stop），或用另一个 JANUS_HOME 启动",
                owner.trim(),
                layout.home().display()
            );
        }
        Err(std::fs::TryLockError::Error(err)) => return Err(err.into()),
    }
    file.set_len(0)?;
    write!(file, "{}", std::process::id())?;
    Ok(file)
}

#[cfg(test)]
mod values_tests {
    use axum::http::{HeaderMap, HeaderValue};

    fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(*k, HeaderValue::from_static(v));
        }
        h
    }

    /// MCP values go to the app on this machine; a web page, or a page that rebinds its own
    /// domain to the loopback address, is refused.
    #[test]
    fn values_go_only_to_a_local_app() {
        for host in ["127.0.0.1:7878", "localhost:7878", "[::1]:7878", "127.0.0.1"] {
            assert!(super::from_local_app(&headers(&[("host", host)])), "{host}");
        }
        assert!(!super::from_local_app(&headers(&[("host", "evil.example:7878")])), "a rebound domain");
        assert!(!super::from_local_app(&headers(&[("host", "127.0.0.1.evil.example")])));
        assert!(!super::from_local_app(&headers(&[("host", "127.0.0.1:7878"), ("origin", "https://evil.example")])));
        assert!(!super::from_local_app(&headers(&[("host", "127.0.0.1:7878"), ("sec-fetch-site", "same-origin")])));
        assert!(!super::from_local_app(&HeaderMap::new()), "no Host");
    }
}

#[cfg(test)]
mod lock_tests {
    /// A second holder of one home is refused while the first lives; the lock goes with its holder.
    #[test]
    fn one_janus_per_home() {
        let home = std::env::temp_dir().join(format!("janus-lock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        let home = crate::layout::Layout::new(home);
        let first = super::lock_home(&home).unwrap();
        assert_eq!(std::fs::read_to_string(home.pid_file()).unwrap(), std::process::id().to_string());
        let err = super::lock_home(&home).unwrap_err().to_string();
        assert!(err.contains("另一个 Janus"), "{err}");
        assert_eq!(
            std::fs::read_to_string(home.pid_file()).unwrap(),
            std::process::id().to_string(),
            "the refused one does not overwrite the owner"
        );
        drop(first);
        // Other tests spawn processes meanwhile: a child forked in that moment holds the descriptor
        // until its exec closes it, so the release can take an instant to show.
        let again = (0..100).find_map(|_| {
            super::lock_home(&home).ok().or_else(|| {
                std::thread::sleep(std::time::Duration::from_millis(20));
                None
            })
        });
        assert!(again.is_some(), "the lock is released with its holder");
        let _ = std::fs::remove_dir_all(home.home());
    }
}

type ApiResult = Result<Response, (StatusCode, String)>;
fn bad(err: anyhow::Error) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, format!("{err:#}"))
}

async fn meta(State(app): State<Arc<App>>) -> Json<Value> {
    Json(json!({
        "kinds": agents::kinds(),
        "local": {
            "codex": agents::which("codex"), "claude": agents::which("claude"),
        },
        "version": VERSION,
        "dataVersion": app.db.data_version().unwrap_or_default(),
        "home": app.cfg.layout.home(),
        "db": app.cfg.layout.db(),
    }))
}

/// Take or decline the newer default text of an edited default role: `{field, adopt}`.
async fn seed_choice(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>, Json(body): Json<Value>) -> ApiResult {
    let field = body["field"].as_str().unwrap_or_default();
    let role = app.seed_choice(&id, field, body["adopt"].as_bool().unwrap_or(false)).map_err(bad)?;
    Ok(Json(role).into_response())
}

/// Everything the setup screens need in one request.
async fn setup(State(app): State<Arc<App>>) -> ApiResult {
    Ok(Json(json!({
        "connections": app.connections().map_err(bad)?,
        "roles": app.roles().map_err(bad)?,
        "workspaces": app.workspaces().map_err(bad)?,
    }))
    .into_response())
}

async fn save(State(app): State<Arc<App>>, UrlPath(kind): UrlPath<String>, Json(body): Json<Value>) -> ApiResult {
    let saved = match kind.as_str() {
        "connections" => app.save_connection(&body),
        "roles" => app.save_role(&body),
        "workspaces" => app.save_workspace(&body),
        _ => return Err((StatusCode::NOT_FOUND, "unknown kind".into())),
    }
    .map_err(bad)?;
    Ok(Json(saved).into_response())
}

async fn remove(State(app): State<Arc<App>>, UrlPath((kind, id)): UrlPath<(String, String)>) -> ApiResult {
    match kind.as_str() {
        "connections" => app.delete_connection(&id),
        "roles" => app.delete_role(&id),
        "workspaces" => app.delete_workspace(&id),
        _ => return Err((StatusCode::NOT_FOUND, "unknown kind".into())),
    }
    .map_err(bad)?;
    Ok(Json(json!({ "ok": true })).into_response())
}

async fn test_connection(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> ApiResult {
    Ok(Json(app.start_check(&id).map_err(bad)?).into_response())
}

async fn login_connection(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> ApiResult {
    app.start_login(&id).map_err(bad)?;
    Ok(Json(json!({ "ok": true })).into_response())
}

#[derive(Deserialize)]
struct PreviewQuery {
    kind: String,
    source: Option<String>,
}

async fn local_preview(Query(q): Query<PreviewQuery>) -> Json<Value> {
    Json(agents::local_preview(&q.kind, q.source.as_deref().unwrap_or("")))
}

/// The ACP adapters: installed versions, the newest known one, and any install in progress.
async fn adapters(State(app): State<Arc<App>>) -> Json<Value> {
    Json(app.adapters())
}

/// `install` (body: optional `version`), `check` for the newest version, or `use` an installed
/// `version`. Installs and checks run in the background; the app polls `GET /api/adapters`.
async fn adapter_action(
    State(app): State<Arc<App>>,
    UrlPath((id, action)): UrlPath<(String, String)>,
    body: Option<Json<Value>>,
) -> ApiResult {
    let body = body.map(|b| b.0).unwrap_or_default();
    let version = body["version"].as_str().filter(|v| !v.is_empty()).map(str::to_owned);
    match action.as_str() {
        "install" => app.start_adapter_install(&id, version),
        "check" => app.start_adapter_check(&id),
        "use" => app.use_adapter_version(&id, version.as_deref().unwrap_or("")),
        _ => return Err((StatusCode::NOT_FOUND, "unknown action".into())),
    }
    .map_err(bad)?;
    Ok(Json(app.adapters()).into_response())
}

fn ok() -> ApiResult {
    Ok(Json(json!({ "ok": true })).into_response())
}

fn reply<T: serde::Serialize>(v: anyhow::Result<T>) -> ApiResult {
    Ok(Json(v.map_err(bad)?).into_response())
}

// @project-doc docs/interfaces/20261031-http_api.md#library
async fn library(State(app): State<Arc<App>>) -> ApiResult {
    reply(app.library_view())
}

async fn library_item(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> ApiResult {
    reply(app.item_view(&id))
}

/// A server's saved values, for the editor. They go only to a client on this machine that isn't
/// a web page: see `from_local_app`.
// @project-doc docs/operations/20261031-security.md#secrets
async fn mcp_values(
    State(app): State<Arc<App>>,
    UrlPath(id): UrlPath<String>,
    headers: axum::http::HeaderMap,
) -> ApiResult {
    if !from_local_app(&headers) {
        return Err((StatusCode::FORBIDDEN, "MCP 的值只返回给本机的 Janus 应用".into()));
    }
    reply(app.mcp_values_view(&id))
}

/// A request from a program on this machine rather than a web page: the Host is a loopback name
/// (a page that rebinds its own domain to 127.0.0.1 still sends that domain), and there is no
/// `Origin` or `Sec-Fetch-Site` (browsers add them to a page's requests; the app sends neither).
fn from_local_app(headers: &axum::http::HeaderMap) -> bool {
    let host = headers.get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("");
    let name = if host.starts_with('[') {
        host.split(']').next().map(|h| format!("{h}]")).unwrap_or_default()
    } else {
        host.split(':').next().unwrap_or("").to_string()
    };
    matches!(name.as_str(), "127.0.0.1" | "localhost" | "[::1]")
        && !headers.contains_key(header::ORIGIN)
        && !headers.contains_key("sec-fetch-site")
}

async fn delete_library_item(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> ApiResult {
    app.delete_item(&id).await.map_err(bad)?;
    ok()
}

async fn refresh_library_item(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> ApiResult {
    reply(app.refresh_item(&id).await)
}

async fn create_skill(State(app): State<Arc<App>>, Json(body): Json<Value>) -> ApiResult {
    reply(app.create_skill(&body).await)
}

async fn edit_skill(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>, Json(body): Json<Value>) -> ApiResult {
    reply(app.edit_skill(&id, &body).await)
}

// @project-doc docs/interfaces/20261031-http_api.md#folder_skills
async fn scan_skill_folder(State(app): State<Arc<App>>, Json(body): Json<Value>) -> ApiResult {
    reply(app.scan_skill_folder(body["path"].as_str().unwrap_or("")))
}

async fn import_skill(State(app): State<Arc<App>>, Json(body): Json<Value>) -> ApiResult {
    let path = body["path"].as_str().unwrap_or("");
    if let Some(paths) = body.get("paths") {
        let paths =
            Vec::<String>::deserialize(paths).map_err(|_| bad(anyhow::anyhow!("Skill 路径必须是字符串列表")))?;
        reply(app.import_skill_selection(path, &paths).await)
    } else {
        reply(app.import_skill_folder(path).await)
    }
}

/// A server the person just added or changed is prepared right away, in the background.
// @project-doc docs/domains/20261116-resource_library.md#mcp_router
fn warmed(app: &Arc<App>, saved: anyhow::Result<library::Item>) -> anyhow::Result<library::Item> {
    if let Ok(item) = &saved {
        app.warm_mcp(&item.id);
    }
    saved
}

async fn create_mcp(State(app): State<Arc<App>>, Json(body): Json<Value>) -> ApiResult {
    reply(warmed(&app, app.save_mcp(None, &body).await))
}

async fn edit_mcp(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>, Json(body): Json<Value>) -> ApiResult {
    reply(warmed(&app, app.save_mcp(Some(&id), &body).await))
}

async fn import_mcp(State(app): State<Arc<App>>, Json(body): Json<Value>) -> ApiResult {
    let imported = app.import_mcp_config(body["text"].as_str().unwrap_or("")).await;
    if let Ok(r) = &imported {
        for id in r["items"].as_array().into_iter().flatten().filter_map(|i| i["id"].as_str()) {
            app.warm_mcp(id);
        }
    }
    reply(imported)
}

async fn warm_mcp(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> ApiResult {
    app.item(&id).map_err(bad)?;
    app.warm_mcp(&id);
    Ok(Json(json!({ "ok": true })).into_response())
}

async fn add_source(State(app): State<Arc<App>>, Json(body): Json<Value>) -> ApiResult {
    reply(app.add_source(body["url"].as_str().unwrap_or("")).await)
}

async fn check_sources(State(app): State<Arc<App>>) -> ApiResult {
    reply(app.check_sources().await)
}

async fn import_from_source(
    State(app): State<Arc<App>>,
    UrlPath(id): UrlPath<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let paths: Vec<String> =
        body["paths"].as_array().into_iter().flatten().filter_map(|p| p.as_str().map(str::to_owned)).collect();
    reply(app.import_from_source(&id, &paths).await)
}

async fn update_source(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> ApiResult {
    reply(app.update_source(&id).await)
}

async fn delete_source(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> ApiResult {
    app.delete_source(&id).await.map_err(bad)?;
    ok()
}

#[derive(Deserialize)]
struct SuggestQuery {
    kind: String,
    name: Option<String>,
}

async fn suggest_id(State(app): State<Arc<App>>, Query(q): Query<SuggestQuery>) -> ApiResult {
    Ok(Json(json!({ "id": app.suggest_agent_id(&q.kind, q.name.as_deref().unwrap_or("")).map_err(bad)? }))
        .into_response())
}

#[derive(Deserialize)]
struct PathQuery {
    path: String,
}

async fn inspect(Query(q): Query<PathQuery>) -> Json<Value> {
    let path = PathBuf::from(expand_home(q.path.trim()));
    if !path.is_dir() {
        return Json(json!({ "exists": false, "path": path }));
    }
    let branch = git(&path, &["rev-parse", "--abbrev-ref", "HEAD"]).await.ok().map(|s| s.trim().to_string());
    let dirty = match branch {
        Some(_) => git(&path, &["status", "--porcelain"]).await.map(|s| s.lines().count()).unwrap_or(0),
        None => 0,
    };
    Json(json!({ "exists": true, "path": path, "git": branch.is_some(), "branch": branch, "dirty": dirty }))
}

async fn role_stats(State(app): State<Arc<App>>) -> ApiResult {
    Ok(Json(app.role_stats().map_err(bad)?).into_response())
}

#[derive(Deserialize)]
struct RunsQuery {
    kind: Option<String>,
    workspace: Option<String>,
    /// `1`: only runs that may need the person (working, delivered, stopped part-way), however old.
    attention: Option<u8>,
    /// `next` of the previous page.
    cursor: Option<String>,
    limit: Option<usize>,
}

/// Runs of one kind, newest first, a page at a time: goals (the default) or chats (`?kind=chat`),
/// of one workspace with `?workspace=`. A page has `limit` runs (50 by default, at most 500) and
/// `next`, the cursor of the page after it (`null` on the last page).
// @project-doc docs/interfaces/20261031-http_api.md#run_list
async fn list_runs(State(app): State<Arc<App>>, Query(q): Query<RunsQuery>) -> ApiResult {
    let kind = match q.kind.as_deref() {
        None => RunKind::Goal,
        Some(k) => RunKind::parse(k).ok_or((StatusCode::BAD_REQUEST, format!("未知的类型：{k}")))?,
    };
    let before = match q.cursor.as_deref() {
        None => None,
        Some(c) => Some(
            c.split_once(':')
                .and_then(|(t, id)| Some((t.parse::<i64>().ok()?, id.to_string())))
                .ok_or((StatusCode::BAD_REQUEST, format!("cursor 不对：{c}")))?,
        ),
    };
    use status::RunStatus::*;
    let statuses =
        (q.attention == Some(1)).then(|| vec![Planning, Running, WaitingInput, Delivered, Interrupted, Failed]);
    let page =
        db::RunsPage { kind, workspace: q.workspace, statuses, before, limit: q.limit.unwrap_or(50).clamp(1, 500) };
    let (runs, more) = app.db.runs_page(&page).map_err(bad)?;
    let next = runs
        .last()
        .filter(|_| more)
        .map(|r| format!("{}:{}", r["createdAt"].as_i64().unwrap_or(0), r["id"].as_str().unwrap_or("")));
    Ok(Json(json!({ "runs": runs, "next": next })).into_response())
}

/// Every workspace that goal runs were made in, as their snapshots name it, deleted ones included.
// @project-doc docs/interfaces/20261031-http_api.md#run_list
async fn run_workspaces(State(app): State<Arc<App>>) -> ApiResult {
    Ok(Json(json!({ "workspaces": app.db.run_workspaces().map_err(bad)? })).into_response())
}

/// Start a conversation with one digital employee; it opens with the first message.
// @project-doc docs/interfaces/20261031-http_api.md#chats
async fn create_chat(State(app): State<Arc<App>>, Json(req): Json<NewChat>) -> ApiResult {
    let id = app.create_chat(req).await.map_err(bad)?;
    Ok(Json(json!({ "id": id })).into_response())
}

#[derive(Deserialize)]
struct PreflightQuery {
    workspace: String,
}

/// What to remind the person of before a run starts in a workspace (they may go ahead anyway).
// @project-doc docs/interfaces/20261031-http_api.md#run_preflight
async fn run_preflight(State(app): State<Arc<App>>, Query(q): Query<PreflightQuery>) -> ApiResult {
    let warnings = app.preflight(&q.workspace).await.map_err(bad)?;
    Ok(Json(json!({ "warnings": warnings })).into_response())
}

async fn create_run(State(app): State<Arc<App>>, Json(req): Json<NewRun>) -> ApiResult {
    let id = app.create_run(req).await.map_err(bad)?;
    Ok(Json(json!({ "id": id })).into_response())
}

/// Remove a run with every record and file Janus kept for it. Only a run that is not working can
/// go; the workspace's own files are not touched.
// @project-doc docs/interfaces/20261031-http_api.md#run_delete
async fn delete_run(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> ApiResult {
    match app.delete_run(&id).await {
        Ok(()) => ok(),
        Err(DeleteRefused::NotFound) => Err((StatusCode::NOT_FOUND, "任务不存在".into())),
        Err(DeleteRefused::Conflict(why)) => Err((StatusCode::CONFLICT, why)),
        Err(DeleteRefused::Invalid(err)) => Err(bad(err)),
    }
}

/// One run and its events; with `after`, only the events past that seq, so a client that already
/// holds the rest (the app's inbox) reads just what is new.
async fn get_run(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>, Query(q): Query<After>) -> ApiResult {
    let run = app.db.run(&id).map_err(bad)?.ok_or((StatusCode::NOT_FOUND, "run not found".into()))?;
    let events = app.db.events(&id, q.after.unwrap_or(0).max(0)).map_err(bad)?;
    Ok(Json(json!({ "run": run, "events": events, "live": app.run(&id).is_some() })).into_response())
}

/// The run's read-only usage ledger over its whole event log (`backend/src/usage.rs`). A run always
/// has its `run.created` event, so no events means no such run.
// @project-doc docs/interfaces/20261031-http_api.md#run_usage
async fn get_usage(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> ApiResult {
    let events = app.db.events(&id, 0).map_err(bad)?;
    if events.is_empty() {
        return Err((StatusCode::NOT_FOUND, "run not found".into()));
    }
    Ok(Json(usage::ledger(&events)).into_response())
}

#[derive(Deserialize)]
struct Period {
    from: i64,
    to: i64,
    /// The person's time zone in minutes east of UTC, so active days are their days.
    offset: Option<i64>,
}

/// The usage bill over a period, across runs (`backend/src/bill.rs`).
async fn usage_bill(State(app): State<Arc<App>>, Query(p): Query<Period>) -> ApiResult {
    const MAX_DAYS: i64 = 400;
    if p.to <= p.from || p.to - p.from > MAX_DAYS * 24 * 60 * 60 * 1000 {
        return Err((StatusCode::BAD_REQUEST, format!("区间要在 1 毫秒到 {MAX_DAYS} 天之间")));
    }
    let prices = app.prices();
    Ok(Json(bill::period(&app.db, p.from, p.to, p.offset.unwrap_or(0), prices.as_deref()).map_err(bad)?)
        .into_response())
}

#[derive(Deserialize)]
struct PriceQuery {
    /// Model names, comma separated, as the sessions name them.
    models: Option<String>,
}

/// List prices of the given models from the synced models.dev list (`backend/src/prices.rs`), so
/// the app can price a task's bill; `null` for a model without a price.
// @project-doc docs/interfaces/20261031-http_api.md#prices
async fn get_prices(State(app): State<Arc<App>>, Query(q): Query<PriceQuery>) -> ApiResult {
    Ok(Json(price_reply(app.prices().as_deref(), q.models.as_deref().unwrap_or(""))).into_response())
}

/// Download the models.dev list now instead of waiting for the daily sync.
async fn sync_prices(State(app): State<Arc<App>>) -> ApiResult {
    let table = app.sync_prices(true).await.map_err(bad)?;
    Ok(Json(price_reply(Some(&table), "")).into_response())
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

/// Files in a run's workspace whose path contains `q`, for the `@` menu of the composer.
// @project-doc docs/interfaces/20261031-http_api.md#run_files
async fn run_files(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>, Query(q): Query<FileQuery>) -> ApiResult {
    let run = app.db.run(&id).map_err(bad)?.ok_or((StatusCode::NOT_FOUND, "run not found".into()))?;
    let cwd = std::path::PathBuf::from(run["cwd"].as_str().unwrap_or_default());
    let files = acp_view::workspace_files(&cwd, q.q.as_deref().unwrap_or(""), 20);
    Ok(Json(json!({ "files": files })).into_response())
}

#[derive(Deserialize)]
struct FileQuery {
    q: Option<String>,
}

#[derive(Deserialize)]
struct After {
    after: Option<i64>,
}

async fn stream(
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
        // A lagged receiver tells the app to reload rather than silently skip events.
        Err(_) => Some(Ok(SseEvent::default().event("lagged").data("{}"))),
        _ => None,
    });
    Sse::new(tokio_stream::iter(backlog).chain(live)).keep_alive(KeepAlive::default())
}

/// Everything the app follows across runs, as it happens: a short notice for each new event of any
/// run (`event`: run, seq, type) and each change of a run's status (`run`: run, status). The app
/// reads what it needs about a run only when that run has news, instead of polling every run.
/// `ready` opens the stream (the app reads its lists once then), `lagged` asks it to read again.
// @project-doc docs/interfaces/20261031-http_api.md#global_stream
async fn stream_all(
    State(app): State<Arc<App>>,
) -> Sse<impl tokio_stream::Stream<Item = Result<SseEvent, Infallible>>> {
    let lagged = || Ok(SseEvent::default().event("lagged").data("{}"));
    let events = BroadcastStream::new(app.tx.subscribe()).map(move |msg| match msg {
        Ok(e) => Ok(SseEvent::default()
            .event("event")
            .data(json!({ "run": e.run_id, "seq": e.seq, "type": e.kind }).to_string())),
        Err(_) => lagged(),
    });
    let changes = BroadcastStream::new(app.changes.subscribe()).map(move |msg| match msg {
        Ok(c) => Ok(SseEvent::default().event("run").data(serde_json::to_string(&c).unwrap_or_default())),
        Err(_) => lagged(),
    });
    let ready = tokio_stream::once(Ok(SseEvent::default().event("ready").data("{}")));
    Sse::new(ready.chain(events.merge(changes))).keep_alive(KeepAlive::default())
}

/// The shared board as an MCP server (streamable HTTP, JSON responses) for the sessions of one
/// run: the lead's orchestration tools, or a collaborator's. The token keeps other runs and
/// other local processes out.
async fn mcp(
    State(app): State<Arc<App>>,
    UrlPath((run, token, who)): UrlPath<(String, String, String)>,
    Json(body): Json<Value>,
) -> ApiResult {
    match app.mcp(&run, &token, &who, body).await {
        Ok(Some(reply)) => Ok(Json(reply).into_response()),
        Ok(None) => Ok(StatusCode::ACCEPTED.into_response()),
        Err(err) => Err((StatusCode::FORBIDDEN, format!("{err:#}"))),
    }
}

/// The run's MCP router (`janus-tools`): the library's command servers for everyone.
// @project-doc docs/domains/20261116-resource_library.md#mcp_router
async fn mcp_tools(
    State(app): State<Arc<App>>,
    UrlPath((run, token, who)): UrlPath<(String, String, String)>,
    Json(body): Json<Value>,
) -> ApiResult {
    match app.mcp_tools(&run, &token, &who, body).await {
        Ok(Some(reply)) => Ok(Json(reply).into_response()),
        Ok(None) => Ok(StatusCode::ACCEPTED.into_response()),
        Err(err) => Err((StatusCode::FORBIDDEN, format!("{err:#}"))),
    }
}

/// No server-initiated stream: every response comes back on its POST.
async fn mcp_stream() -> StatusCode {
    StatusCode::METHOD_NOT_ALLOWED
}

async fn action(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>, Json(body): Json<Value>) -> ApiResult {
    match app.request_action(&id, &body).await {
        Ok(ActionReply::Done) => Ok(Json(json!({ "ok": true })).into_response()),
        Ok(ActionReply::Duplicate) => {
            Ok(Json(json!({ "ok": true, "duplicate": true, "commandId": body["commandId"] })).into_response())
        }
        Err(ActionRefused::NotFound) => Err((StatusCode::NOT_FOUND, "run not found".into())),
        Err(ActionRefused::Conflict(why)) => Err((StatusCode::CONFLICT, why)),
        Err(ActionRefused::Invalid(err)) => Err(bad(err)),
    }
}

async fn export(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> ApiResult {
    let lines: Vec<String> =
        app.db.events(&id, 0).map_err(bad)?.iter().map(|e| serde_json::to_string(e).unwrap()).collect();
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
async fn file(State(app): State<Arc<App>>, Query(q): Query<PathQuery>) -> ApiResult {
    let path = PathBuf::from(&q.path);
    if !path.exists() {
        return Err((StatusCode::NOT_FOUND, "not found".into()));
    }
    if !app.cfg.layout.is_artifact(&path) {
        return Err((StatusCode::FORBIDDEN, "只能读取 Janus 的产物".into()));
    }
    let text = std::fs::read_to_string(&path).map_err(|e| bad(e.into()))?;
    Ok(([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], text).into_response())
}

/// The release this build is: the date version a package build sets (`JANUS_VERSION`), else
/// the crate version. The app compares it with its own to tell a stale service.
pub const VERSION: &str = match option_env!("JANUS_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

// @project-doc docs/architecture/20261116-modules.md#startup
#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--version" | "-V") => {
            println!("janus {VERSION}");
            return Ok(());
        }
        Some("migrate") => {
            match args.next().as_deref() {
                Some("--dry-run") => {
                    print!("{}", migrations::dry_run(&layout::current_db(&layout::Layout::new(janus_home()?)))?)
                }
                Some("--shape") => println!("{}", serde_json::to_string_pretty(&data_shape::snapshot()?)?),
                _ => anyhow::bail!(
                    "janus migrate --dry-run 预览这份数据升级到数据版本 {} 会改什么（不修改数据）；janus migrate --shape 输出当前的数据结构快照。升级本身在服务启动时自动执行。",
                    migrations::latest()
                ),
            }
            return Ok(());
        }
        Some("--help" | "-h") => {
            println!(
                "janus {}: local multi-agent workspace. Run without arguments to start the server.\n`janus migrate --dry-run` previews the data upgrade without changing anything.\n\nSettings (environment, else ~/.janus/config.toml): JANUS_HOME, JANUS_ADDR (addr), JANUS_NODE (node), JANUS_MAX_CONCURRENT_ATTEMPTS (max_concurrent_attempts), JANUS_RUN_TOKEN_BUDGET (run_token_budget), JANUS_RUN_COST_BUDGET_USD (run_cost_budget_usd).\nSee docs/operations/20261031-deployment.md.",
                VERSION
            );
            return Ok(());
        }
        Some(other) => anyhow::bail!("unknown argument {other}; see janus --help"),
        None => {}
    }
    let layout = layout::Layout::new(janus_home()?);
    let _home_lock = lock_home(&layout)?;
    // Nothing runs yet that could be using the scratch of the last run.
    let _ = std::fs::remove_dir_all(layout.tmp());
    layout::adopt_db(&layout)?;
    let (db, upgrade) = db::Db::open(&layout)?;
    if !upgrade.applied.is_empty() {
        let backup = upgrade.backup.as_ref().map(|b| format!("，升级前的备份：{}", b.display())).unwrap_or_default();
        config::log(format!("数据从版本 {} 升级到 {}{backup}", upgrade.from, upgrade.to));
        for (version, name, changed) in &upgrade.applied {
            config::log(format!("  {version}「{name}」：改动 {changed} 处"));
        }
    }
    let foreign = layout.foreign();
    if !foreign.is_empty() {
        config::log(format!("Janus home 中这些不是 Janus 的：{}（Janus 不读也不删它们）", foreign.join("、")));
    }
    let (tx, _) = tokio::sync::broadcast::channel(4096);
    let (changes, _) = tokio::sync::broadcast::channel(1024);
    let settings = config::settings();
    let app = Arc::new(App {
        db,
        cfg: Config {
            layout,
            addr: settings.addr.clone(),
            node: settings.node.clone(),
            stall_after: orchestrator::STALL_AFTER,
            goal_settle: goal_settle::SETTLE,
            retry_after: orchestrator::RETRY_AFTER,
            max_concurrent_attempts: settings.max_concurrent_attempts,
            run_token_budget: settings.run_token_budget,
            run_cost_budget_usd: settings.run_cost_budget_usd,
        },
        tx,
        changes,
        runs: Mutex::new(HashMap::new()),
        adapter_state: Default::default(),
        checks: Default::default(),
        prices: Default::default(),
        library: Default::default(),
        revival: Default::default(),
        worktree_adds: Default::default(),
    });
    app.recover()?;
    app.sync_seeds()?;
    app.reset_stale_checks()?;
    // Library sources: ask their remotes for news shortly after start, then every few hours.
    {
        let app = app.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            loop {
                if let Err(err) = app.check_sources().await {
                    config::log(format!("janus: library sources not checked: {err:#}"));
                }
                tokio::time::sleep(library::CHECK_EVERY).await;
            }
        });
    }

    // ACP adapters: ask npm for newer versions shortly after start, then every few hours; the app
    // reminds the person to upgrade.
    // @project-doc docs/architecture/20261031-adapters.md#update_check
    {
        let app = app.clone();
        tokio::spawn(async move {
            tokio::time::sleep(adapters::FIRST_CHECK_AFTER).await;
            loop {
                let checking = app.clone();
                if let Err(err) = tokio::task::spawn_blocking(move || checking.check_adapter_updates()).await {
                    config::log(format!("janus: adapter update check stopped: {err}"));
                }
                tokio::time::sleep(adapters::CHECK_EVERY).await;
            }
        });
    }

    // Model prices: check every hour, download models.dev's list again once a day.
    {
        let app = app.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            loop {
                if let Err(err) = app.sync_prices(false).await {
                    config::log(format!("janus: model prices not synced: {err:#}"));
                }
                tokio::time::sleep(prices::CHECK_EVERY).await;
            }
        });
    }

    // @project-doc docs/interfaces/20261031-http_api.md#routes
    let router = Router::new()
        .route("/api/meta", get(meta))
        .route("/api/inspect", get(inspect))
        .route("/api/local-config", get(local_preview))
        .route("/api/adapters", get(adapters))
        .route("/api/adapters/{id}/{action}", post(adapter_action))
        .route("/api/library", get(library))
        .route("/api/library/items/{id}", get(library_item).delete(delete_library_item))
        .route("/api/library/items/{id}/refresh", post(refresh_library_item))
        .route("/api/library/skills", post(create_skill))
        .route("/api/library/skills/scan", post(scan_skill_folder))
        .route("/api/library/skills/import", post(import_skill))
        .route("/api/library/skills/{id}", axum::routing::put(edit_skill))
        .route("/api/library/mcp", post(create_mcp))
        .route("/api/library/mcp/import", post(import_mcp))
        .route("/api/library/mcp/{id}", axum::routing::put(edit_mcp))
        .route("/api/library/mcp/{id}/warm", post(warm_mcp))
        .route("/api/library/mcp/{id}/values", get(mcp_values))
        .route("/api/library/sources", post(add_source))
        .route("/api/library/sources/check", post(check_sources))
        .route("/api/library/sources/{id}", delete(delete_source))
        .route("/api/library/sources/{id}/import", post(import_from_source))
        .route("/api/library/sources/{id}/update", post(update_source))
        .route("/api/agent-id", get(suggest_id))
        .route("/api/setup", get(setup))
        .route("/api/setup/{kind}", post(save))
        .route("/api/setup/{kind}/{id}", delete(remove))
        .route("/api/roles/{id}/seed", post(seed_choice))
        .route("/api/connections/{id}/test", post(test_connection))
        .route("/api/connections/{id}/login", post(login_connection))
        .route("/api/file", get(file))
        .route("/api/stats/roles", get(role_stats))
        .route("/api/usage", get(usage_bill))
        .route("/api/prices", get(get_prices))
        .route("/api/prices/sync", post(sync_prices))
        .route("/api/runs", get(list_runs).post(create_run))
        .route("/api/chats", post(create_chat))
        .route("/api/runs/preflight", get(run_preflight))
        .route("/api/runs/workspaces", get(run_workspaces))
        .route("/api/runs/{id}", get(get_run).delete(delete_run))
        .route("/api/runs/{id}/stream", get(stream))
        .route("/api/stream", get(stream_all))
        .route("/api/runs/{id}/actions", post(action))
        .route("/api/runs/{id}/export", get(export))
        .route("/api/runs/{id}/usage", get(get_usage))
        .route("/api/runs/{id}/files", get(run_files))
        .route("/mcp/{run}/{token}/{who}", post(mcp).get(mcp_stream))
        .route("/mcp/{run}/{token}/{who}/tools", post(mcp_tools).get(mcp_stream))
        // Goals and messages carry pasted images as base64 (10 images of up to 10 MB each).
        .layer(axum::extract::DefaultBodyLimit::max(160 * 1024 * 1024))
        .with_state(app.clone());

    let addr = &app.cfg.addr;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    config::log(format!("janus {VERSION}: http://{addr}  ·  home {}", app.cfg.layout.home().display()));
    axum::serve(listener, router).await?;
    Ok(())
}
