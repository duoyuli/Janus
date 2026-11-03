mod acp;
mod adapters;
mod agents;
mod config;
mod db;
mod mcp_sources;
mod orchestrator;
mod skill_sources;

use anyhow::Result;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::{StatusCode, header};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use orchestrator::{App, Config, NewRun, expand_home, git};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;

/// Project root: the checkout that holds examples/ (the sample workspace offered on first use).
/// Nothing is written here.
fn root() -> PathBuf {
    std::env::var("JANUS_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".."))
}

/// Janus home (~/.janus, like Lody's ~/.lody): every user-level thing Janus keeps. Owner-only,
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
/// startup (`recover`) and drive the same database. `<home>/janus.pid` names the owner while it lives.
// @project-doc docs/operations/deployment.md#single_instance
fn lock_home(home: &std::path::Path) -> Result<()> {
    let file = home.join("janus.pid");
    let me = std::process::id();
    if let Some(pid) = std::fs::read_to_string(&file).ok().and_then(|t| t.trim().parse::<u32>().ok()) {
        let alive = pid != me
            && std::process::Command::new("kill")
                .args(["-0", &pid.to_string()])
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
        if alive {
            anyhow::bail!(
                "另一个 Janus（pid {pid}）正在使用 {}。先停止它（以服务方式运行时执行 just stop），或用另一个 JANUS_HOME 启动",
                home.display()
            );
        }
    }
    std::fs::write(&file, me.to_string())?;
    Ok(())
}

type ApiResult = Result<Response, (StatusCode, String)>;
fn bad(err: anyhow::Error) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, format!("{err:#}"))
}

async fn meta(State(app): State<Arc<App>>) -> Json<Value> {
    let home = std::env::var("HOME").unwrap_or_default();
    let has = |p: &str| PathBuf::from(&home).join(p).exists();
    Json(json!({
        "kinds": agents::kinds(),
        "local": {
            "codex": agents::which("codex"), "claude": agents::which("claude"), "pi": agents::which("pi"),
            "piConfig": has(".pi/agent/models.json") || has(".pi/agent/auth.json"),
        },
        "dataDir": app.cfg.data_dir,
        "agentsDir": app.cfg.agents_dir,
        "defaultCwd": root().join("examples/calc").canonicalize().ok(),
        "db": app.cfg.agents_dir.parent().map(|h| h.join("janus.db")),
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

async fn save(
    State(app): State<Arc<App>>,
    UrlPath(kind): UrlPath<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let saved = match kind.as_str() {
        "connections" => app.save_connection(&body),
        "roles" => app.save_role(&body),
        "workspaces" => app.save_workspace(&body),
        _ => return Err((StatusCode::NOT_FOUND, "unknown kind".into())),
    }
    .map_err(bad)?;
    Ok(Json(saved).into_response())
}

async fn remove(
    State(app): State<Arc<App>>,
    UrlPath((kind, id)): UrlPath<(String, String)>,
) -> ApiResult {
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
    Json(agents::local_preview(
        &q.kind,
        q.source.as_deref().unwrap_or(""),
    ))
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

async fn skills(State(app): State<Arc<App>>) -> Json<Value> {
    Json(app.all_skills())
}

async fn skill_sources(State(app): State<Arc<App>>) -> Json<Value> {
    Json(app.list_skill_sources())
}

async fn add_skill_source(State(app): State<Arc<App>>, Json(body): Json<Value>) -> ApiResult {
    Ok(Json(app.add_skill_source(body["url"].as_str().unwrap_or("")).await.map_err(bad)?).into_response())
}

async fn check_skill_sources(State(app): State<Arc<App>>) -> Json<Value> {
    Json(app.check_skill_sources().await)
}

async fn update_skill_source(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> ApiResult {
    Ok(Json(app.update_skill_source(&id).await.map_err(bad)?).into_response())
}

async fn delete_skill_source(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> ApiResult {
    app.delete_skill_source(&id).map_err(bad)?;
    Ok(Json(json!({ "ok": true })).into_response())
}

async fn mcp_sources(State(app): State<Arc<App>>) -> Json<Value> {
    Json(app.list_mcp_sources())
}

async fn add_mcp_source(State(app): State<Arc<App>>, Json(body): Json<Value>) -> ApiResult {
    Ok(Json(app.add_mcp_source(body["url"].as_str().unwrap_or("")).await.map_err(bad)?).into_response())
}

async fn check_mcp_sources(State(app): State<Arc<App>>) -> Json<Value> {
    Json(app.check_mcp_sources().await)
}

async fn update_mcp_source(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> ApiResult {
    Ok(Json(app.update_mcp_source(&id).await.map_err(bad)?).into_response())
}

async fn delete_mcp_source(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> ApiResult {
    app.delete_mcp_source(&id).map_err(bad)?;
    Ok(Json(json!({ "ok": true })).into_response())
}

/// Set or clear one resource's values. The reply lists the keys, never the values.
async fn mcp_resource_secrets(
    State(app): State<Arc<App>>,
    UrlPath((id, key)): UrlPath<(String, String)>,
    Json(body): Json<Value>,
) -> ApiResult {
    Ok(Json(app.write_mcp_secret(&format!("{id}/{key}"), &body).map_err(bad)?).into_response())
}

#[derive(Deserialize)]
struct SuggestQuery {
    kind: String,
    name: Option<String>,
}

async fn suggest_id(State(app): State<Arc<App>>, Query(q): Query<SuggestQuery>) -> ApiResult {
    Ok(Json(json!({ "id": app.suggest_agent_id(&q.kind, q.name.as_deref().unwrap_or("")).map_err(bad)? })).into_response())
}

async fn create_skill(Json(body): Json<Value>) -> ApiResult {
    Ok(Json(agents::create_skill(&body).map_err(bad)?).into_response())
}

async fn import_skill(Json(body): Json<Value>) -> ApiResult {
    Ok(
        Json(agents::import_skill(body["path"].as_str().unwrap_or("")).map_err(bad)?)
            .into_response(),
    )
}

async fn delete_skill(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> ApiResult {
    app.delete_skill(&id).map_err(bad)?;
    Ok(Json(json!({ "ok": true })).into_response())
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
    let branch = git(&path, &["rev-parse", "--abbrev-ref", "HEAD"])
        .await
        .ok()
        .map(|s| s.trim().to_string());
    let dirty = match branch {
        Some(_) => git(&path, &["status", "--porcelain"])
            .await
            .map(|s| s.lines().count())
            .unwrap_or(0),
        None => 0,
    };
    Json(
        json!({ "exists": true, "path": path, "git": branch.is_some(), "branch": branch, "dirty": dirty }),
    )
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

async fn get_run(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> ApiResult {
    let run = app
        .db
        .runs()
        .map_err(bad)?
        .into_iter()
        .find(|r| r["id"] == id.as_str())
        .ok_or((StatusCode::NOT_FOUND, "run not found".into()))?;
    let events = app.db.events(&id, 0).map_err(bad)?;
    Ok(
        Json(json!({ "run": run, "events": events, "live": app.run(&id).is_some() }))
            .into_response(),
    )
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
    let last = backlog
        .last()
        .map(|e| e.seq)
        .unwrap_or(q.after.unwrap_or(0));
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

async fn action(
    State(app): State<Arc<App>>,
    UrlPath(id): UrlPath<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let command_id = body["commandId"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned);
    if let Some(command_id) = command_id.as_deref()
        && app.db.command_exists(&id, command_id).map_err(bad)? {
            return Ok(
                Json(json!({ "ok": true, "duplicate": true, "commandId": command_id }))
                    .into_response(),
            );
        }
    if let Some(expected) = body["expectedSequence"].as_i64() {
        let actual = app.db.max_seq(&id).map_err(bad)?;
        if expected != actual {
            return Err((
                StatusCode::CONFLICT,
                format!("状态已变化：expectedSequence={expected}，当前为 {actual}"),
            ));
        }
    }
    let run = match app.run(&id) {
        Some(run) => run,
        None => match body["type"].as_str().unwrap_or("") {
            // Acceptance needs no live agents, so it also works for runs delivered before a restart.
            "accept" => {
                app.accept_archived(&id).map_err(bad)?;
                return Ok(Json(json!({ "ok": true })).into_response());
            }
            // Continuing, or simply talking to an agent, brings an interrupted run back.
            "resumeRun" => {
                app.resume_run(&id).await.map_err(bad)?;
                return Ok(Json(json!({ "ok": true })).into_response());
            }
            "message" => app.resume_run(&id).await.map_err(bad)?,
            _ => {
                return Err((
                    StatusCode::CONFLICT,
                    "这个任务已经中断：先点「从中断处继续」，或者给某个 Agent 发消息".into(),
                ));
            }
        },
    };
    if body["type"] == "resumeRun" {
        // The run is live (a failed driver removes its run, see launch), so there is nothing to resume.
        return Err((
            StatusCode::CONFLICT,
            "这个 Run 正在进行，不需要从中断处继续".into(),
        ));
    }
    app.action(&run, &body).map_err(bad)?;
    if let Some(command_id) = command_id {
        app.db
            .record_command(
                &id,
                &command_id,
                &json!({ "accepted": true, "type": body["type"] }),
                orchestrator::now_ms(),
            )
            .map_err(bad)?;
    }
    Ok(Json(json!({ "ok": true })).into_response())
}

async fn export(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> ApiResult {
    let lines: Vec<String> = app
        .db
        .events(&id, 0)
        .map_err(bad)?
        .iter()
        .map(|e| serde_json::to_string(e).unwrap())
        .collect();
    Ok((
        [
            (header::CONTENT_TYPE, "application/x-ndjson; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                &format!("attachment; filename=\"janus-{id}.jsonl\""),
            ),
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
    let path = PathBuf::from(&q.path)
        .canonicalize()
        .map_err(|_| (StatusCode::NOT_FOUND, "not found".into()))?;
    if !path.starts_with(&base) {
        return Err((
            StatusCode::FORBIDDEN,
            "只能读取 Janus 的产物目录".into(),
        ));
    }
    let text = std::fs::read_to_string(&path).map_err(|e| bad(e.into()))?;
    Ok(([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], text).into_response())
}

// @project-doc docs/architecture/overview.md#startup
#[tokio::main]
async fn main() -> Result<()> {
    match std::env::args().nth(1).as_deref() {
        Some("--version" | "-V") => {
            println!("janus {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some("--help" | "-h") => {
            println!("janus {}: local multi-agent workspace. Run without arguments to start the server.\n\nSettings (environment, else ~/.janus/config.toml): JANUS_HOME, JANUS_ADDR (addr), JANUS_NODE (node).\nSee docs/operations/deployment.md.", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some(other) => anyhow::bail!("unknown argument {other}; see janus --help"),
        None => {}
    }
    let home = janus_home()?;
    lock_home(&home)?;
    let db_path = home.join("janus.db");
    let db = db::Db::open(&db_path)?;
    let (tx, _) = tokio::sync::broadcast::channel(4096);
    let agents_dir = home.join("agents");
    std::fs::create_dir_all(&agents_dir)?;
    let app = Arc::new(App {
        db,
        cfg: Config {
            data_dir: home.clone(),
            agents_dir,
            stall_after: orchestrator::STALL_AFTER,
        },
        tx,
        runs: Mutex::new(HashMap::new()),
    });
    app.recover()?;
    app.seed_roles()?;
    app.reset_stale_checks()?;
    // Skill sources: ask their remotes for news shortly after start, then every few hours.
    {
        let app = app.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            loop {
                app.check_skill_sources().await;
                tokio::time::sleep(skill_sources::CHECK_EVERY).await;
            }
        });
    }
    // MCP sources: same cadence, on their own loop so a slow Skill check never delays them.
    {
        let app = app.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(45)).await;
            loop {
                app.check_mcp_sources().await;
                tokio::time::sleep(mcp_sources::CHECK_EVERY).await;
            }
        });
    }

    // @project-doc docs/interfaces/http_api.md#routes
    let router = Router::new()
        .route("/api/meta", get(meta))
        .route("/api/inspect", get(inspect))
        .route("/api/local-config", get(local_preview))
        .route("/api/adapters", get(adapters))
        .route("/api/adapters/{id}/{action}", post(adapter_action))
        .route("/api/skills", get(skills).post(create_skill))
        .route("/api/skills/import", post(import_skill))
        .route("/api/skills/{id}", delete(delete_skill))
        .route("/api/skill-sources", get(skill_sources).post(add_skill_source))
        .route("/api/skill-sources/check", post(check_skill_sources))
        .route("/api/skill-sources/{id}", delete(delete_skill_source))
        .route("/api/skill-sources/{id}/update", post(update_skill_source))
        .route("/api/mcp-sources", get(mcp_sources).post(add_mcp_source))
        .route("/api/mcp-sources/check", post(check_mcp_sources))
        .route("/api/mcp-sources/{id}", delete(delete_mcp_source))
        .route("/api/mcp-sources/{id}/update", post(update_mcp_source))
        .route("/api/mcp-sources/{id}/servers/{key}/secrets", post(mcp_resource_secrets))
        .route("/api/agent-id", get(suggest_id))
        .route("/api/setup", get(setup))
        .route("/api/setup/{kind}", post(save))
        .route("/api/setup/{kind}/{id}", delete(remove))
        .route("/api/connections/{id}/test", post(test_connection))
        .route("/api/connections/{id}/login", post(login_connection))
        .route("/api/file", get(file))
        .route("/api/stats/roles", get(role_stats))
        .route("/api/runs", get(list_runs).post(create_run))
        .route("/api/runs/{id}", get(get_run))
        .route("/api/runs/{id}/stream", get(stream))
        .route("/api/runs/{id}/actions", post(action))
        .route("/api/runs/{id}/export", get(export))
        .route("/mcp/{run}/{token}/{who}", post(mcp).get(mcp_stream))
        .with_state(app);

    let addr = &config::settings().addr;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    config::log(format!("janus {}: http://{addr}  ·  home {}", env!("CARGO_PKG_VERSION"), home.display()));
    axum::serve(listener, router).await?;
    Ok(())
}
