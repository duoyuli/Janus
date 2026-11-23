//! The HTTP API the app talks to, and the MCP endpoints the agents' sessions reach. Handlers only
//! translate between HTTP and `App`: they read the request, call one `App` method and shape its
//! answer. Every failure is an `ApiError`, a status with a message meant to be shown as it is.
//!
//! The routes are grouped by what they serve: `setup` (agents, roles, workspaces, adapters),
//! `library`, `runs` (runs, chats, usage, files) and `streams` (event streams and MCP).

use crate::orchestrator::{App, Refused};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde_json::json;
use std::sync::Arc;

mod library;
mod loops;
mod memory;
mod runs;
mod setup;
mod streams;

/// A refused request: its status and a message in the person's words
/// (`docs/interfaces/20261031-http_api.md#errors`). Any `anyhow::Error` becomes a 400 with its
/// whole chain, so a handler can use `?` on what `App` returns.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> ApiError {
        ApiError { status, message: message.into() }
    }

    fn bad_request(message: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::BAD_REQUEST, message)
    }

    fn not_found(message: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::NOT_FOUND, message)
    }

    fn forbidden(message: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::FORBIDDEN, message)
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(err: anyhow::Error) -> ApiError {
        ApiError::bad_request(format!("{err:#}"))
    }
}

/// A run that is not there is 404, a state that does not allow the request 409, anything else 400.
impl From<Refused> for ApiError {
    fn from(refused: Refused) -> ApiError {
        match refused {
            Refused::NotFound => ApiError::not_found("任务不存在"),
            Refused::Conflict(why) => ApiError::new(StatusCode::CONFLICT, why),
            Refused::Invalid(err) => err.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, self.message).into_response()
    }
}

type ApiResult = Result<Response, ApiError>;

/// `v` as the JSON body of a 200 response.
fn reply(v: impl serde::Serialize) -> ApiResult {
    Ok(Json(v).into_response())
}

/// The body of a request that changed something and has nothing else to say.
fn ok() -> ApiResult {
    reply(json!({ "ok": true }))
}

/// A request from a program on this machine rather than a web page: the Host is a loopback name
/// (a page that rebinds its own domain to 127.0.0.1 still sends that domain), and there is no
/// `Origin` or `Sec-Fetch-Site` (browsers add them to a page's requests; the app sends neither).
/// What only the app may read (MCP values, an agent's environment) goes only to such a client.
// @project-doc docs/operations/20261031-security.md#secrets
fn from_local_app(headers: &HeaderMap) -> bool {
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

/// Refuse a request for what only the app may read unless it comes from the app (`from_local_app`).
fn local_app_only(headers: &HeaderMap, what: &str) -> Result<(), ApiError> {
    if from_local_app(headers) {
        Ok(())
    } else {
        Err(ApiError::forbidden(format!("{what}只返回给本机的 Janus 应用")))
    }
}

/// Every route Janus serves.
// @project-doc docs/interfaces/20261031-http_api.md#routes
pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/api/meta", get(setup::meta))
        .route("/api/inspect", get(setup::inspect))
        .route("/api/local-config", get(setup::local_preview))
        .route("/api/adapters", get(setup::adapters))
        .route("/api/adapters/{id}/{action}", post(setup::adapter_action))
        .route("/api/library", get(library::library))
        .route("/api/library/items/{id}", get(library::item).delete(library::delete_item))
        .route("/api/library/items/{id}/refresh", post(library::refresh_item))
        .route("/api/library/skills", post(library::create_skill))
        .route("/api/library/skills/scan", post(library::scan_skill_folder))
        .route("/api/library/skills/import", post(library::import_skill))
        .route("/api/library/skills/{id}", put(library::edit_skill))
        .route("/api/library/mcp", post(library::create_mcp))
        .route("/api/library/mcp/import", post(library::import_mcp))
        .route("/api/library/mcp/{id}", put(library::edit_mcp))
        .route("/api/library/mcp/{id}/warm", post(library::warm_mcp))
        .route("/api/library/mcp/{id}/values", get(library::mcp_values))
        .route("/api/library/sources", post(library::add_source))
        .route("/api/library/sources/check", post(library::check_sources))
        .route("/api/library/sources/{id}", delete(library::delete_source))
        .route("/api/library/sources/{id}/import", post(library::import_from_source))
        .route("/api/library/sources/{id}/update", post(library::update_source))
        .route("/api/agent-id", get(setup::suggest_id))
        .route("/api/host-env", get(setup::host_env).put(setup::save_host_env))
        .route("/api/host-env/try", post(setup::try_host_command))
        .route("/api/setup", get(setup::setup))
        .route("/api/setup/{kind}", post(setup::save))
        .route("/api/setup/{kind}/{id}", delete(setup::remove))
        .route("/api/roles/{id}/seed", post(setup::seed_choice))
        .route("/api/connections/{id}/test", post(setup::test_connection))
        .route("/api/connections/{id}/env", get(setup::connection_env))
        .route(
            "/api/roles/{id}/avatar",
            get(setup::role_avatar).put(setup::set_role_avatar).delete(setup::remove_role_avatar),
        )
        .route("/api/connections/{id}/login", post(setup::login_connection))
        .route("/api/file", get(runs::file))
        .route("/api/stats/roles", get(setup::role_stats))
        .route("/api/usage", get(runs::usage_bill))
        .route("/api/prices", get(runs::prices))
        .route("/api/prices/sync", post(runs::sync_prices))
        .route("/api/runs", get(runs::list).post(runs::create))
        .route("/api/chats", post(runs::create_chat))
        .route("/api/loops", get(loops::list))
        .route("/api/loops/schedule", get(loops::schedule))
        .route("/api/workspaces/{id}/loops", post(loops::create))
        .route("/api/loops/{id}", get(loops::get).put(loops::update).delete(loops::remove))
        .route("/api/loops/{id}/{action}", post(loops::act))
        .route("/api/workspaces/{id}/memories", get(memory::list))
        .route("/api/workspaces/{id}/memories/consolidate", post(memory::consolidate))
        .route("/api/memories/{id}", put(memory::update).delete(memory::remove))
        .route("/api/runs/preflight", get(runs::preflight))
        .route("/api/runs/workspaces", get(runs::workspaces))
        .route("/api/runs/{id}", get(runs::get).delete(runs::remove))
        .route("/api/runs/{id}/stream", get(streams::run))
        .route("/api/stream", get(streams::all))
        .route("/api/runs/{id}/actions", post(runs::action))
        .route("/api/runs/{id}/export", get(runs::export))
        .route("/api/runs/{id}/usage", get(runs::usage))
        .route("/api/runs/{id}/files", get(runs::files))
        .route("/mcp/{run}/{token}/{who}", post(streams::board).get(streams::no_stream))
        .route("/mcp/{run}/{token}/{who}/tools", post(streams::tools).get(streams::no_stream))
        // Goals and messages carry pasted images as base64 (10 images of up to 10 MB each).
        .layer(axum::extract::DefaultBodyLimit::max(160 * 1024 * 1024))
        .with_state(app)
}

#[cfg(test)]
mod tests {
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

    /// A missing run is 404, a run whose state does not allow the request 409, anything else 400;
    /// the message stays as `App` wrote it.
    #[test]
    fn refusals_keep_their_status_and_message() {
        use super::{ApiError, Refused};
        use axum::http::StatusCode;
        let answer = |r: Refused| {
            let e = ApiError::from(r);
            (e.status, e.message)
        };
        assert_eq!(answer(Refused::NotFound), (StatusCode::NOT_FOUND, "任务不存在".into()));
        assert_eq!(answer(Refused::Conflict("还在进行".into())), (StatusCode::CONFLICT, "还在进行".into()));
        let invalid = Refused::Invalid(anyhow::anyhow!("没有这个子任务").context("重试失败"));
        assert_eq!(answer(invalid), (StatusCode::BAD_REQUEST, "重试失败: 没有这个子任务".into()));
    }
}
