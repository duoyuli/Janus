//! Setup: agents (connections), roles, workspaces, the ACP adapters, and what the setup screens
//! read about this machine.

use super::{ApiError, ApiResult, local_app_only, ok, reply};
use crate::agents;
use crate::orchestrator::App;
use crate::util::expand_home;
use crate::wire::wire_enum;
use axum::Json;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::{HeaderMap, header};
use axum::response::IntoResponse;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;

wire_enum! {
    /// The kinds of setup object `/api/setup/{kind}` saves and deletes.
    enum SetupKind {
        Connections = "connections",
        Roles = "roles",
        Workspaces = "workspaces",
    }
}

impl SetupKind {
    fn of(kind: &str) -> Result<SetupKind, ApiError> {
        SetupKind::parse(kind).ok_or_else(|| ApiError::not_found(format!("没有这类设置：{kind}")))
    }
}

wire_enum! {
    /// What `/api/adapters/{id}/{action}` does with an adapter.
    enum AdapterAction {
        /// Install a version (the body's, else the newest). On a pinned adapter, this resumes
        /// automatic updates: the pin goes, and the newest version installed is switched to
        /// without downloading it again.
        Install = "install",
        /// Switch to an installed version and pin it: automatic updates leave it alone.
        Use = "use",
    }
}

pub(super) async fn meta(State(app): State<Arc<App>>) -> ApiResult {
    reply(json!({
        "kinds": agents::kinds(),
        "local": {
            "codex": agents::which("codex"), "claude": agents::which("claude"),
        },
        "version": crate::VERSION,
        "dataVersion": app.db.data_version().unwrap_or_default(),
        "home": app.cfg.layout.home(),
        "db": app.cfg.layout.db(),
        // An app update waits until this is empty: restarting the server would cut these runs off.
        "atWork": app.at_work(),
    }))
}

/// Take or decline the newer default text of an edited default role: `{field, adopt}`.
pub(super) async fn seed_choice(
    State(app): State<Arc<App>>,
    UrlPath(id): UrlPath<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let field = body["field"].as_str().unwrap_or_default();
    reply(app.seed_choice(&id, field, body["adopt"].as_bool().unwrap_or(false))?)
}

/// Everything the setup screens need in one request.
pub(super) async fn setup(State(app): State<Arc<App>>) -> ApiResult {
    reply(json!({
        "connections": app.connections()?,
        "roles": app.roles()?,
        "workspaces": app.workspaces()?,
    }))
}

/// What of the person's home agents do not see. It names what is in the home folder and decides
/// what agents can reach, so only the app on this Mac may use it.
// @project-doc docs/operations/20261031-security.md#host_env
pub(super) async fn host_env(State(app): State<Arc<App>>, headers: HeaderMap) -> ApiResult {
    local_app_only(&headers, "本机环境")?;
    reply(app.host_env()?)
}

// @project-doc docs/domains/20261031-goal_mode.md#prompt_language
pub(super) async fn language(State(app): State<Arc<App>>) -> ApiResult {
    reply(json!({ "language": app.language() }))
}

pub(super) async fn save_language(State(app): State<Arc<App>>, Json(body): Json<Value>) -> ApiResult {
    reply(app.save_language(&body)?)
}

pub(super) async fn save_host_env(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> ApiResult {
    local_app_only(&headers, "本机环境")?;
    reply(app.save_host_env(&body)?)
}

/// Run a command as the person and as an agent would, to compare: the app on this Mac only, as it
/// runs what it is given.
pub(super) async fn try_host_command(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> ApiResult {
    local_app_only(&headers, "试命令")?;
    reply(app.try_host_command(body["command"].as_str().unwrap_or("")).await?)
}

pub(super) async fn save(
    State(app): State<Arc<App>>,
    UrlPath(kind): UrlPath<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    reply(match SetupKind::of(&kind)? {
        SetupKind::Connections => app.save_connection(&body)?,
        SetupKind::Roles => json!(app.save_role(&input(body)?)?),
        SetupKind::Workspaces => json!(app.save_workspace(&input(body)?)?),
    })
}

/// A setting as the editor sends it, or a 400 that says what does not fit.
fn input<T: serde::de::DeserializeOwned>(body: Value) -> Result<T, ApiError> {
    serde_json::from_value(body).map_err(|e| ApiError::bad_request(format!("设置的内容不对：{e}")))
}

pub(super) async fn remove(State(app): State<Arc<App>>, UrlPath((kind, id)): UrlPath<(String, String)>) -> ApiResult {
    match SetupKind::of(&kind)? {
        SetupKind::Connections => app.delete_connection(&id),
        SetupKind::Roles => app.delete_role(&id),
        SetupKind::Workspaces => app.delete_workspace(&id).await,
    }?;
    ok()
}

pub(super) async fn test_connection(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> ApiResult {
    reply(app.start_check(&id)?)
}

pub(super) async fn login_connection(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> ApiResult {
    app.start_login(&id)?;
    ok()
}

/// An agent's environment with every value, private ones included, for the editor. Like MCP
/// values, it goes only to a client on this machine that isn't a web page: see `from_local_app`.
// @project-doc docs/operations/20261031-security.md#secrets
pub(super) async fn connection_env(
    State(app): State<Arc<App>>,
    UrlPath(id): UrlPath<String>,
    headers: HeaderMap,
) -> ApiResult {
    local_app_only(&headers, "Agent 的环境变量")?;
    reply(app.connection_env(&id)?)
}

/// A digital employee's picture, PNG; 404 when it shows its initials.
pub(super) async fn role_avatar(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> ApiResult {
    match app.role_avatar(&id)? {
        Some(png) => Ok(([(header::CONTENT_TYPE, "image/png")], png).into_response()),
        None => Err(ApiError::not_found("这个数字员工没有头像")),
    }
}

/// Set a digital employee's picture: `{"data": "<PNG, base64>"}`. Returns the role.
pub(super) async fn set_role_avatar(
    State(app): State<Arc<App>>,
    UrlPath(id): UrlPath<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let data = body["data"].as_str().ok_or_else(|| ApiError::bad_request("缺少头像数据 data"))?;
    reply(app.set_role_avatar(&id, Some(data))?)
}

/// Take a digital employee back to its initials. Returns the role.
pub(super) async fn remove_role_avatar(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> ApiResult {
    reply(app.set_role_avatar(&id, None)?)
}

/// The role files at a folder (or a file) the person chose: `{path}`.
// @project-doc docs/domains/20261031-agents_and_roles.md#role_files
pub(super) async fn scan_role_files(State(app): State<Arc<App>>, Json(body): Json<Value>) -> ApiResult {
    reply(app.scan_role_files(&chosen_path(&body)?)?)
}

/// Make digital employees of the chosen role files: `{path, files, connection}`, `files` as the
/// scan listed them. Returns the new employees.
pub(super) async fn import_roles(State(app): State<Arc<App>>, Json(body): Json<Value>) -> ApiResult {
    let files =
        Vec::<String>::deserialize(&body["files"]).map_err(|_| ApiError::bad_request("files 必须是文件路径的列表"))?;
    reply(app.import_roles(&chosen_path(&body)?, &files, body["connection"].as_str().unwrap_or(""))?)
}

fn chosen_path(body: &Value) -> Result<PathBuf, ApiError> {
    match body["path"].as_str().map(str::trim) {
        Some(p) if !p.is_empty() => Ok(PathBuf::from(expand_home(p))),
        _ => Err(ApiError::bad_request("缺少 path")),
    }
}

/// A digital employee as a role file, `text/markdown`, for export.
pub(super) async fn role_file(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> ApiResult {
    Ok(([(header::CONTENT_TYPE, "text/markdown; charset=utf-8")], app.role_file(&id)?).into_response())
}

/// Rename a group of digital employees: `{from, to}`.
pub(super) async fn rename_role_group(State(app): State<Arc<App>>, Json(body): Json<Value>) -> ApiResult {
    let text = |k: &str| body[k].as_str().unwrap_or("").to_string();
    reply(json!({ "moved": app.rename_role_group(&text("from"), &text("to"))? }))
}

/// Put a whole group on one agent: `{group, connection}` (`group` empty: those in no group).
pub(super) async fn bind_role_group(State(app): State<Arc<App>>, Json(body): Json<Value>) -> ApiResult {
    let text = |k: &str| body[k].as_str().unwrap_or("").to_string();
    reply(app.bind_role_group(&text("group"), &text("connection"))?)
}

#[derive(Deserialize)]
pub(super) struct PreviewQuery {
    kind: String,
    source: Option<String>,
}

pub(super) async fn local_preview(Query(q): Query<PreviewQuery>) -> ApiResult {
    reply(agents::local_preview(&q.kind, q.source.as_deref().unwrap_or(""))?)
}

/// The ACP adapters: installed versions, the newest known one, and any install in progress.
pub(super) async fn adapters(State(app): State<Arc<App>>) -> ApiResult {
    reply(app.adapters())
}

/// `install` (body: optional `version`) or `use` an installed `version`. Installs run in the
/// background; the app follows `GET /api/adapters`. `use` pins the version; `install` on a pinned
/// adapter resumes automatic updates.
pub(super) async fn adapter_action(
    State(app): State<Arc<App>>,
    UrlPath((id, action)): UrlPath<(String, String)>,
    body: Option<Json<Value>>,
) -> ApiResult {
    let action = AdapterAction::parse(&action).ok_or_else(|| ApiError::not_found(format!("没有这个操作：{action}")))?;
    let body = body.map(|b| b.0).unwrap_or_default();
    let version = body["version"].as_str().filter(|v| !v.is_empty()).map(str::to_owned);
    match action {
        AdapterAction::Install => app.start_adapter_install(&id, version),
        AdapterAction::Use => app.use_adapter_version(&id, version.as_deref().unwrap_or("")),
    }?;
    reply(app.adapters())
}

#[derive(Deserialize)]
pub(super) struct SuggestQuery {
    kind: String,
    name: Option<String>,
}

pub(super) async fn suggest_id(State(app): State<Arc<App>>, Query(q): Query<SuggestQuery>) -> ApiResult {
    reply(json!({ "id": app.suggest_agent_id(&q.kind, q.name.as_deref().unwrap_or(""))? }))
}

#[derive(Deserialize)]
pub(super) struct PathQuery {
    pub(super) path: String,
}

/// What the workspace editor shows about a folder: whether it exists, and its Git branch and
/// uncommitted changes.
pub(super) async fn inspect(Query(q): Query<PathQuery>) -> ApiResult {
    let path = PathBuf::from(expand_home(q.path.trim()));
    if !path.is_dir() {
        return reply(json!({ "exists": false, "path": path }));
    }
    let branch =
        crate::git::query(&path, &["rev-parse", "--abbrev-ref", "HEAD"]).await.ok().map(|s| s.trim().to_string());
    let dirty = match branch {
        Some(_) => crate::git::query(&path, &["status", "--porcelain"]).await.map(|s| s.lines().count()).unwrap_or(0),
        None => 0,
    };
    reply(json!({ "exists": true, "path": path, "git": branch.is_some(), "branch": branch, "dirty": dirty }))
}

pub(super) async fn role_stats(State(app): State<Arc<App>>) -> ApiResult {
    reply(app.role_stats()?)
}
