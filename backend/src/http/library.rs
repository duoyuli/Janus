//! The resource library: skills, MCP servers and the Git sources they come from.
// @project-doc docs/interfaces/20261031-http_api.md#library

use super::{ApiError, ApiResult, Client, local_app_only, ok, reply};
use crate::library::Item;
use crate::orchestrator::App;
use axum::Json;
use axum::extract::{Extension, Path as UrlPath, State};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

pub(super) async fn library(State(app): State<Arc<App>>) -> ApiResult {
    reply(app.library_view()?)
}

/// The MCP router's settings: whether it gives `run_code` (code mode).
// @project-doc docs/interfaces/20261031-http_api.md#library
pub(super) async fn save_router(State(app): State<Arc<App>>, Json(body): Json<Value>) -> ApiResult {
    reply(app.save_code_mode(&body)?)
}

pub(super) async fn item(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> ApiResult {
    reply(app.item_view(&id)?)
}

/// A server's saved values, for the editor. They go only to a client on this machine that isn't
/// a web page: see `from_local_app`.
// @project-doc docs/operations/20261031-security.md#secrets
pub(super) async fn mcp_values(
    State(app): State<Arc<App>>,
    UrlPath(id): UrlPath<String>,
    Extension(client): Extension<Client>,
) -> ApiResult {
    local_app_only(&client, "MCP 的值")?;
    reply(app.mcp_values_view(&id)?)
}

pub(super) async fn delete_item(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> ApiResult {
    app.delete_item(&id).await?;
    ok()
}

pub(super) async fn refresh_item(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> ApiResult {
    reply(app.refresh_item(&id).await?)
}

pub(super) async fn create_skill(State(app): State<Arc<App>>, Json(body): Json<Value>) -> ApiResult {
    reply(app.create_skill(&body).await?)
}

pub(super) async fn edit_skill(
    State(app): State<Arc<App>>,
    UrlPath(id): UrlPath<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    reply(app.edit_skill(&id, &body).await?)
}

// @project-doc docs/interfaces/20261031-http_api.md#folder_skills
pub(super) async fn scan_skill_folder(State(app): State<Arc<App>>, Json(body): Json<Value>) -> ApiResult {
    reply(app.scan_skill_folder(body["path"].as_str().unwrap_or(""))?)
}

pub(super) async fn import_skill(State(app): State<Arc<App>>, Json(body): Json<Value>) -> ApiResult {
    let path = body["path"].as_str().unwrap_or("");
    match body.get("paths") {
        Some(paths) => {
            let paths =
                Vec::<String>::deserialize(paths).map_err(|_| ApiError::bad_request("Skill 路径必须是字符串列表"))?;
            reply(app.import_skill_selection(path, &paths).await?)
        }
        None => reply(app.import_skill_folder(path).await?),
    }
}

/// A server the person just added or changed is prepared right away, in the background.
// @project-doc docs/domains/20261116-resource_library.md#mcp_router
fn warmed(app: &Arc<App>, saved: anyhow::Result<Item>) -> ApiResult {
    let item = saved?;
    app.warm_mcp(&item.id);
    reply(item)
}

pub(super) async fn create_mcp(State(app): State<Arc<App>>, Json(body): Json<Value>) -> ApiResult {
    warmed(&app, app.save_mcp(None, &body).await)
}

pub(super) async fn edit_mcp(
    State(app): State<Arc<App>>,
    UrlPath(id): UrlPath<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    warmed(&app, app.save_mcp(Some(&id), &body).await)
}

pub(super) async fn import_mcp(State(app): State<Arc<App>>, Json(body): Json<Value>) -> ApiResult {
    let imported = app.import_mcp_config(body["text"].as_str().unwrap_or("")).await?;
    for id in imported["items"].as_array().into_iter().flatten().filter_map(|i| i["id"].as_str()) {
        app.warm_mcp(id);
    }
    reply(imported)
}

pub(super) async fn warm_mcp(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> ApiResult {
    app.item(&id)?;
    app.warm_mcp(&id);
    ok()
}

pub(super) async fn add_source(State(app): State<Arc<App>>, Json(body): Json<Value>) -> ApiResult {
    reply(app.add_source(body["url"].as_str().unwrap_or("")).await?)
}

pub(super) async fn check_sources(State(app): State<Arc<App>>) -> ApiResult {
    reply(app.check_sources().await?)
}

pub(super) async fn import_from_source(
    State(app): State<Arc<App>>,
    UrlPath(id): UrlPath<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let paths: Vec<String> =
        body["paths"].as_array().into_iter().flatten().filter_map(|p| p.as_str().map(str::to_owned)).collect();
    reply(app.import_from_source(&id, &paths).await?)
}

pub(super) async fn update_source(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> ApiResult {
    reply(app.update_source(&id).await?)
}

pub(super) async fn delete_source(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> ApiResult {
    app.delete_source(&id).await?;
    ok()
}
