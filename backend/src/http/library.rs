//! The resource library: skills, MCP servers and the Git sources they come from.
// @project-doc docs/interfaces/20261031-http_api.md#library

use super::{ApiError, ApiResult, ok, reply};
use crate::library::Item;
use crate::orchestrator::App;
use axum::Json;
use axum::extract::{Path as UrlPath, Query, State};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Deserialize)]
pub(super) struct LangQuery {
    /// `zh` or `en`, for what the library says about its items; the app's language without.
    lang: Option<String>,
}

/// The library, with what the screens say about each item and source in `lang`
/// (`setup_view::library`).
pub(super) async fn library(State(app): State<Arc<App>>, Query(q): Query<LangQuery>) -> ApiResult {
    let lang = super::views::lang_of(&app, q.lang.as_deref())?;
    let mut lib = app.library_view()?;
    crate::setup_view::library::dress_library(&mut lib, lang);
    reply(lib)
}

/// The MCP router's settings: whether it gives `run_code` (code mode).
// @project-doc docs/interfaces/20261031-http_api.md#library
pub(super) async fn save_router(State(app): State<Arc<App>>, Json(body): Json<Value>) -> ApiResult {
    reply(app.save_code_mode(&body)?)
}

pub(super) async fn item(
    State(app): State<Arc<App>>,
    UrlPath(id): UrlPath<String>,
    Query(q): Query<LangQuery>,
) -> ApiResult {
    let lang = super::views::lang_of(&app, q.lang.as_deref())?;
    let mut item = app.item_view(&id)?;
    crate::setup_view::library::dress_item(&mut item, lang);
    reply(item)
}

/// A server's saved values, for the editor: to the app on this Mac and to paired devices, the only
/// clients `devices::guard` lets reach any route.
// @project-doc docs/operations/20261031-security.md#secrets
pub(super) async fn mcp_values(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> ApiResult {
    reply(app.mcp_values_view(&id)?)
}

/// A saved server as the MCP editor starts from it, with its values: the draft, written as a
/// command line and as JSON.
// @project-doc docs/domains/20261116-resource_library.md#mcp
pub(super) async fn mcp_draft(
    State(app): State<Arc<App>>,
    UrlPath(id): UrlPath<String>,
    Query(q): Query<LangQuery>,
) -> ApiResult {
    use crate::setup_view::mcp_draft;
    let lang = super::views::lang_of(&app, q.lang.as_deref())?;
    let values = app.mcp_values_view(&id)?;
    let draft = mcp_draft::Draft::of_item(&app.item_view(&id)?, &values["values"]);
    reply(mcp_draft::edit(draft, None, None, lang))
}

/// One edit in the MCP editor: `{draft, line?, text?}`, the draft with a typed command line or
/// JSON, comes back as the editor draws it (`setup_view::mcp_draft::edit`).
pub(super) async fn edit_mcp_draft(
    State(app): State<Arc<App>>,
    Query(q): Query<LangQuery>,
    Json(body): Json<Value>,
) -> ApiResult {
    use crate::setup_view::mcp_draft;
    let lang = super::views::lang_of(&app, q.lang.as_deref())?;
    let draft: mcp_draft::Draft = serde_json::from_value(body["draft"].clone()).unwrap_or_default();
    reply(mcp_draft::edit(draft, body["line"].as_str(), body["text"].as_str(), lang))
}

/// The community's picks on the shelf of each library tab, with what the library has already.
// @project-doc docs/domains/20261116-resource_library.md#catalog
pub(super) async fn picks(State(app): State<Arc<App>>, Query(q): Query<LangQuery>) -> ApiResult {
    let lang = super::views::lang_of(&app, q.lang.as_deref())?;
    reply(crate::setup_view::picks::shelf(&app.library_view()?, lang))
}

/// Add a pick (`{kind: "skill" | "mcp", id}`). A skill is imported from its GitHub repository,
/// through the library's source for the whole repository when there is one, else by adding that
/// source first. A server that needs nothing is added at once; one that needs a key comes back as
/// `editor`, the draft and what to fill in, for the MCP editor to open on. `message` says what was
/// done.
// @project-doc docs/domains/20261116-resource_library.md#catalog
pub(super) async fn add_pick(
    State(app): State<Arc<App>>,
    Query(q): Query<LangQuery>,
    Json(body): Json<Value>,
) -> ApiResult {
    use crate::lang::tr;
    use crate::setup_view::{mcp_draft, picks};
    let lang = super::views::lang_of(&app, q.lang.as_deref())?;
    let id = body["id"].as_str().unwrap_or("");
    let names = |items: &[Item]| items.iter().map(|i| i.name.clone()).collect::<Vec<_>>().join(lang.pick("、", ", "));
    match body["kind"].as_str() {
        Some("skill") => {
            let s = picks::SKILLS.iter().find(|s| s.id() == id).ok_or_else(|| ApiError::not_found("没有这个推荐"))?;
            let have = app.library_view()?["sources"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|x| {
                    x["repo"].as_str().is_some_and(|r| r.eq_ignore_ascii_case(s.repo))
                        && x["path"] == ""
                        && x["gitRef"] == ""
                })
                .cloned();
            let source = match have {
                Some(src) => src,
                None => app.add_source(&format!("https://github.com/{}", s.repo)).await?,
            };
            if !source["offers"].as_array().into_iter().flatten().any(|o| o["path"] == s.path) {
                let (repo, at) = (s.repo, s.location());
                return Err(ApiError::bad_request(tr!(
                    lang,
                    "{repo} 里没有找到 {at}，可以在来源里「更新」后再试",
                    "{at} isn't in {repo}. Update the source and try again."
                )));
            }
            let source_id = source["id"].as_str().unwrap_or("");
            let imported = app.import_from_source(source_id, &[s.path.to_string()]).await?;
            let what = names(&imported);
            reply(
                json!({ "items": imported, "message": tr!(lang, "已导入 {what}，全员可用", "Imported {what} · available to everyone") }),
            )
        }
        Some("mcp") => {
            let s = picks::SERVERS.iter().find(|s| s.name == id).ok_or_else(|| ApiError::not_found("没有这个推荐"))?;
            let draft = s.draft(lang);
            if !s.one_click() {
                let mut editor = mcp_draft::edit(draft, None, None, lang);
                editor["note"] = json!(s.needs(lang));
                return reply(json!({ "editor": editor }));
            }
            let item = app.save_mcp(None, &draft.body()).await?;
            app.warm_mcp(&item.id);
            let what = names(std::slice::from_ref(&item));
            reply(
                json!({ "items": [item], "message": tr!(lang, "已添加 {what}，全员可用", "Added {what} · available to everyone") }),
            )
        }
        _ => Err(ApiError::bad_request("kind 只能是 skill 或 mcp")),
    }
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
