//! Setup: agents (connections), roles, workspaces, the ACP adapters, and what the setup screens
//! read about this machine.

use super::{ApiError, ApiResult, Client, local_app_only, ok, reply};
use crate::agents;
use crate::orchestrator::App;
use crate::util::expand_home;
use crate::wire::wire_enum;
use axum::Json;
use axum::extract::{Extension, Path as UrlPath, Query, State};
use axum::http::header;
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
        // Where an agent's own folder goes, home-relative: the agent editor names `<agentsDir>/<id>`.
        "agentsDir": crate::run_view::docs::tilde(&app.cfg.layout.agents().to_string_lossy()),
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

#[derive(Deserialize)]
pub(super) struct SetupQuery {
    /// `zh` or `en`, for the reasons and problems written here; the app's language without.
    lang: Option<String>,
}

/// Everything the setup screens need in one request: each role with why it cannot run, each
/// workspace with what keeps its team from running, and the team a new workspace starts with.
// @project-doc docs/interfaces/20261031-http_api.md#setup
pub(super) async fn setup(State(app): State<Arc<App>>, Query(q): Query<SetupQuery>) -> ApiResult {
    use crate::agents::readiness;
    let lang = super::views::lang_of(&app, q.lang.as_deref())?;
    let connections = app.connections()?;
    let mut roles = app.roles()?;
    for r in roles.iter_mut() {
        r["unavailable"] = json!(readiness::unavailable(r, &connections, lang));
        r["permLabel"] = json!(readiness::perm_label(r, &connections, lang));
        r["setupLine"] = json!(readiness::setup_line(r, &connections, lang));
        r["canLead"] = json!(readiness::can_lead(r, &connections));
    }
    let workspaces: Vec<Value> = app
        .workspaces()?
        .into_iter()
        .map(|w| {
            let mut v = json!(w);
            v["problems"] = json!(readiness::problems(&v, &roles, &connections, lang));
            v["teamLine"] = json!(readiness::team_line(&v, &roles, lang));
            v["briefStatus"] = json!(readiness::brief_status(&v, lang));
            v
        })
        .collect();
    let team = readiness::default_team(&roles, &connections);
    let onboarding = readiness::onboarding(&connections, &roles, workspaces.len(), lang);
    let role_groups = readiness::role_groups(&roles, lang);
    for r in roles.iter_mut() {
        crate::setup_view::dress_role(r, &connections, lang);
    }
    let kinds = agents::kinds();
    let mut connections = connections;
    for c in connections.iter_mut() {
        crate::setup_view::dress_connection(c, &kinds, lang);
    }
    reply(json!({
        "connections": connections, "roles": roles, "workspaces": workspaces, "team": team,
        "onboarding": onboarding, "roleGroups": role_groups,
    }))
}

/// What of the person's home agents do not see. It names what is in the home folder and decides
/// what agents can reach, so only the app on this Mac may use it.
// @project-doc docs/operations/20261031-security.md#host_env
pub(super) async fn host_env(State(app): State<Arc<App>>, Extension(client): Extension<Client>) -> ApiResult {
    local_app_only(&client, "本机环境")?;
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
    Extension(client): Extension<Client>,
    Json(body): Json<Value>,
) -> ApiResult {
    local_app_only(&client, "本机环境")?;
    reply(app.save_host_env(&body)?)
}

/// Run a command as the person and as an agent would, to compare: the app on this Mac only, as it
/// runs what it is given.
pub(super) async fn try_host_command(
    State(app): State<Arc<App>>,
    Extension(client): Extension<Client>,
    Json(body): Json<Value>,
) -> ApiResult {
    local_app_only(&client, "试命令")?;
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
/// values, it goes to the app on this Mac and to paired devices only (`devices::guard`).
// @project-doc docs/operations/20261031-security.md#secrets
pub(super) async fn connection_env(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> ApiResult {
    let mut env = app.connection_env(&id)?;
    env["envText"] = json!(crate::setup_view::env_text(&env["env"]));
    reply(env)
}

/// A digital employee's picture, PNG; 404 when it shows its initials.
pub(super) async fn role_avatar(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> ApiResult {
    match app.role_avatar(&id)? {
        Some(png) => Ok(([(header::CONTENT_TYPE, "image/png")], png).into_response()),
        None => Err(ApiError::not_found("这个数字员工没有头像")),
    }
}

/// Set a digital employee's picture: `{"data": "<the image file, base64>"}`, cropped square here.
/// Returns the role.
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
/// The adapters, each with its runtime line in `lang` (`setup_view::adapters`).
fn adapters_in(app: &App, lang: crate::lang::Lang) -> Value {
    let mut a = app.adapters();
    crate::setup_view::adapters::dress(&mut a, crate::util::now_ms() as f64, lang);
    a
}

pub(super) async fn adapters(State(app): State<Arc<App>>, Query(q): Query<SetupQuery>) -> ApiResult {
    let lang = super::views::lang_of(&app, q.lang.as_deref())?;
    reply(adapters_in(&app, lang))
}

/// `install` (body: optional `version`) or `use` an installed `version`. Installs run in the
/// background; the app follows `GET /api/adapters`. `use` pins the version; `install` on a pinned
/// adapter resumes automatic updates.
pub(super) async fn adapter_action(
    State(app): State<Arc<App>>,
    UrlPath((id, action)): UrlPath<(String, String)>,
    Query(q): Query<SetupQuery>,
    body: Option<Json<Value>>,
) -> ApiResult {
    let lang = super::views::lang_of(&app, q.lang.as_deref())?;
    let action = AdapterAction::parse(&action).ok_or_else(|| ApiError::not_found(format!("没有这个操作：{action}")))?;
    let body = body.map(|b| b.0).unwrap_or_default();
    let version = body["version"].as_str().filter(|v| !v.is_empty()).map(str::to_owned);
    match action {
        AdapterAction::Install => app.start_adapter_install(&id, version),
        AdapterAction::Use => app.use_adapter_version(&id, version.as_deref().unwrap_or("")),
    }?;
    reply(adapters_in(&app, lang))
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

#[derive(Deserialize)]
pub(super) struct InspectQuery {
    path: String,
    /// `zh` or `en`, for the line written here; the app's language without.
    lang: Option<String>,
}

/// What the workspace editor shows about a folder: whether it exists, its Git branch and
/// uncommitted changes, and that in a line (`text`) written in `?lang=`.
pub(super) async fn inspect(State(app): State<Arc<App>>, Query(q): Query<InspectQuery>) -> ApiResult {
    use crate::lang::tr;
    let l = super::views::lang_of(&app, q.lang.as_deref())?;
    let path = PathBuf::from(expand_home(q.path.trim()));
    if !path.is_dir() {
        return reply(
            json!({ "exists": false, "path": path, "text": l.pick("目录不存在", "Directory does not exist") }),
        );
    }
    let branch =
        crate::git::query(&path, &["rev-parse", "--abbrev-ref", "HEAD"]).await.ok().map(|s| s.trim().to_string());
    let dirty = match branch {
        Some(_) => crate::git::query(&path, &["status", "--porcelain"]).await.map(|s| s.lines().count()).unwrap_or(0),
        None => 0,
    };
    let text = match &branch {
        None => l
            .pick(
                "不是 Git 仓库：直接改这个目录，没有 diff",
                "Not a Git repository: work happens right in this folder, with no diff",
            )
            .to_string(),
        Some(b) => {
            let tree = if dirty > 0 {
                tr!(
                    l,
                    "⚠ 有 {dirty} 个未提交改动（不会带进任务）",
                    "⚠ Uncommitted changes: {dirty}. Tasks start without them."
                )
            } else {
                l.pick("工作区干净", "Clean working tree").to_string()
            };
            tr!(l, "✓ Git 仓库 · 当前分支 {b} · {tree}", "✓ Git repository · Branch: {b} · {tree}")
        }
    };
    reply(
        json!({ "exists": true, "path": path, "git": branch.is_some(), "branch": branch, "dirty": dirty, "text": text }),
    )
}

/// How each role's subtasks went in recent runs, with the line its editor shows (`line`).
pub(super) async fn role_stats(State(app): State<Arc<App>>, Query(q): Query<SetupQuery>) -> ApiResult {
    let lang = super::views::lang_of(&app, q.lang.as_deref())?;
    let mut stats = app.role_stats()?;
    let runs = stats["runs"].as_i64().unwrap_or(0);
    for x in stats["roles"].as_object_mut().into_iter().flatten().map(|(_, x)| x) {
        x["line"] = json!(crate::setup_view::stat_line(x, runs, lang));
    }
    reply(stats)
}

/// What the digital employee editor shows of the draft it edits: whether it may lead, and its
/// Agent's controls with the draft's values (`setup_view::role_draft`).
pub(super) async fn role_draft(
    State(app): State<Arc<App>>,
    Query(q): Query<SetupQuery>,
    Json(draft): Json<Value>,
) -> ApiResult {
    let lang = super::views::lang_of(&app, q.lang.as_deref())?;
    reply(crate::setup_view::role_draft(&draft, &app.connections()?, lang))
}

/// An agent's environment text with its auth's template brought in (`{text, kind, auth}`): the
/// template's lines for variables the text does not set yet, after what is there.
pub(super) async fn env_template(Json(body): Json<Value>) -> ApiResult {
    let kinds = agents::kinds();
    let template = kinds
        .as_array()
        .and_then(|k| k.iter().find(|k| k["id"] == body["kind"]))
        .and_then(|k| k["auths"].as_array()?.iter().find(|a| a["id"] == body["auth"]))
        .and_then(|a| a["template"].as_str())
        .ok_or_else(|| ApiError::bad_request("这种认证方式没有模板"))?;
    let text = body["text"].as_str().unwrap_or("");
    reply(json!({ "text": crate::setup_view::with_template(text, template) }))
}
