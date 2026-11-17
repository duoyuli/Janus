mod acp;
mod acp_view;
mod adapters;
mod agents;
mod attachments;
mod bill;
mod config;
mod db;
mod evidence;
mod git_probe;
mod goal_settle;
mod library;
mod orchestrator;
mod prices;
mod status;
mod teamwork;
mod traces;
mod usage;

use anyhow::Result;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::{StatusCode, header};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use orchestrator::{ActionRefused, ActionReply, App, Config, DeleteRefused, NewRun, expand_home, git};
use serde::Deserialize;
use serde_json::{Value, json};
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
fn lock_home(home: &std::path::Path) -> Result<std::fs::File> {
    use std::io::Write;
    let mut file =
        std::fs::OpenOptions::new().create(true).truncate(false).read(true).write(true).open(home.join("janus.pid"))?;
    match file.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => {
            let owner = std::fs::read_to_string(home.join("janus.pid")).unwrap_or_default();
            anyhow::bail!(
                "另一个 Janus（pid {}）正在使用 {}。先停止它（以服务方式运行时执行 just stop），或用另一个 JANUS_HOME 启动",
                owner.trim(),
                home.display()
            );
        }
        Err(std::fs::TryLockError::Error(err)) => return Err(err.into()),
    }
    file.set_len(0)?;
    write!(file, "{}", std::process::id())?;
    Ok(file)
}

#[cfg(test)]
mod lock_tests {
    /// A second holder of one home is refused while the first lives; the lock goes with its holder.
    #[test]
    fn one_janus_per_home() {
        let home = std::env::temp_dir().join(format!("janus-lock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        let first = super::lock_home(&home).unwrap();
        assert_eq!(std::fs::read_to_string(home.join("janus.pid")).unwrap(), std::process::id().to_string());
        let err = super::lock_home(&home).unwrap_err().to_string();
        assert!(err.contains("另一个 Janus"), "{err}");
        assert_eq!(
            std::fs::read_to_string(home.join("janus.pid")).unwrap(),
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
        let _ = std::fs::remove_dir_all(&home);
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
        "dataDir": app.cfg.data_dir,
        "agentsDir": app.cfg.agents_dir,
        "db": app.cfg.data_dir.join("janus.db"),
    }))
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

async fn import_skill(State(app): State<Arc<App>>, Json(body): Json<Value>) -> ApiResult {
    reply(app.import_skill_folder(body["path"].as_str().unwrap_or("")).await)
}

async fn create_mcp(State(app): State<Arc<App>>, Json(body): Json<Value>) -> ApiResult {
    reply(app.save_mcp(None, &body).await)
}

async fn edit_mcp(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>, Json(body): Json<Value>) -> ApiResult {
    reply(app.save_mcp(Some(&id), &body).await)
}

async fn import_mcp(State(app): State<Arc<App>>, Json(body): Json<Value>) -> ApiResult {
    reply(app.import_mcp_config(body["text"].as_str().unwrap_or("")).await)
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

async fn list_runs(State(app): State<Arc<App>>) -> ApiResult {
    Ok(Json(json!({ "runs": app.db.runs().map_err(bad)? })).into_response())
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

/// Artifacts are served read-only, and only from the artifacts directory: the rest of Janus home
/// holds the database and credentials.
async fn file(State(app): State<Arc<App>>, Query(q): Query<PathQuery>) -> ApiResult {
    let base = app.cfg.data_dir.join("artifacts");
    std::fs::create_dir_all(&base).map_err(|e| bad(e.into()))?;
    let base = base.canonicalize().map_err(|e| bad(e.into()))?;
    let path = PathBuf::from(&q.path).canonicalize().map_err(|_| (StatusCode::NOT_FOUND, "not found".into()))?;
    if !path.starts_with(&base) {
        return Err((StatusCode::FORBIDDEN, "只能读取 Janus 的产物目录".into()));
    }
    let text = std::fs::read_to_string(&path).map_err(|e| bad(e.into()))?;
    Ok(([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], text).into_response())
}

// @project-doc docs/architecture/20261116-modules.md#startup
#[tokio::main]
async fn main() -> Result<()> {
    match std::env::args().nth(1).as_deref() {
        Some("--version" | "-V") => {
            println!("janus {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some("--help" | "-h") => {
            println!(
                "janus {}: local multi-agent workspace. Run without arguments to start the server.\n\nSettings (environment, else ~/.janus/config.toml): JANUS_HOME, JANUS_ADDR (addr), JANUS_NODE (node).\nSee docs/operations/20261031-deployment.md.",
                env!("CARGO_PKG_VERSION")
            );
            return Ok(());
        }
        Some(other) => anyhow::bail!("unknown argument {other}; see janus --help"),
        None => {}
    }
    let home = janus_home()?;
    let _home_lock = lock_home(&home)?;
    let db_path = home.join("janus.db");
    let db = db::Db::open(&db_path)?;
    let (tx, _) = tokio::sync::broadcast::channel(4096);
    let agents_dir = home.join("agents");
    std::fs::create_dir_all(&agents_dir)?;
    let settings = config::settings();
    let app = Arc::new(App {
        db,
        cfg: Config {
            data_dir: home.clone(),
            agents_dir,
            addr: settings.addr.clone(),
            node: settings.node.clone(),
            stall_after: orchestrator::STALL_AFTER,
            goal_settle: goal_settle::SETTLE,
        },
        tx,
        runs: Mutex::new(HashMap::new()),
        adapter_state: Default::default(),
        prices: Default::default(),
        library: Default::default(),
    });
    app.recover()?;
    app.seed_roles()?;
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
        .route("/api/library/skills/import", post(import_skill))
        .route("/api/library/skills/{id}", axum::routing::put(edit_skill))
        .route("/api/library/mcp", post(create_mcp))
        .route("/api/library/mcp/import", post(import_mcp))
        .route("/api/library/mcp/{id}", axum::routing::put(edit_mcp))
        .route("/api/library/sources", post(add_source))
        .route("/api/library/sources/check", post(check_sources))
        .route("/api/library/sources/{id}", delete(delete_source))
        .route("/api/library/sources/{id}/import", post(import_from_source))
        .route("/api/library/sources/{id}/update", post(update_source))
        .route("/api/agent-id", get(suggest_id))
        .route("/api/setup", get(setup))
        .route("/api/setup/{kind}", post(save))
        .route("/api/setup/{kind}/{id}", delete(remove))
        .route("/api/connections/{id}/test", post(test_connection))
        .route("/api/connections/{id}/login", post(login_connection))
        .route("/api/file", get(file))
        .route("/api/stats/roles", get(role_stats))
        .route("/api/usage", get(usage_bill))
        .route("/api/prices", get(get_prices))
        .route("/api/prices/sync", post(sync_prices))
        .route("/api/runs", get(list_runs).post(create_run))
        .route("/api/runs/{id}", get(get_run).delete(delete_run))
        .route("/api/runs/{id}/stream", get(stream))
        .route("/api/runs/{id}/actions", post(action))
        .route("/api/runs/{id}/export", get(export))
        .route("/api/runs/{id}/usage", get(get_usage))
        .route("/api/runs/{id}/files", get(run_files))
        .route("/mcp/{run}/{token}/{who}", post(mcp).get(mcp_stream))
        // Goals and messages carry pasted images as base64 (10 images of up to 10 MB each).
        .layer(axum::extract::DefaultBodyLimit::max(160 * 1024 * 1024))
        .with_state(app.clone());

    let addr = &app.cfg.addr;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    config::log(format!("janus {}: http://{addr}  ·  home {}", env!("CARGO_PKG_VERSION"), home.display()));
    axum::serve(listener, router).await?;
    Ok(())
}
