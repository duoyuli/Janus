//! Who may ask Janus anything: the app on this Mac, or a browser or phone paired with it. Every
//! request passes `guard`, which lets the app's through, asks every other client for its device
//! cookie, refuses a browser's request that another site made, and keeps the network listener to
//! the person's API (the agents' MCP endpoints stay on this Mac). Also the pairing routes and the
//! network switch.
// @project-doc docs/operations/20261031-security.md#paired_devices

use super::{ApiError, ApiResult, Client, Via, from_local_app, local_app_only, ok, reply};
use crate::devices::{COOKIE, ClaimRefused};
use crate::orchestrator::App;
use axum::Json;
use axum::body::{Body, Bytes};
use axum::extract::{ConnectInfo, Extension, Path as UrlPath, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

/// How long the device cookie lives: the longest a browser keeps one (400 days).
const COOKIE_MAX_AGE: i64 = 400 * 24 * 3600;

fn unauthorized(message: &str) -> Response {
    ApiError::new(StatusCode::UNAUTHORIZED, message).into_response()
}

/// The token in the request's device cookie, if it has one.
fn cookie_token(headers: &HeaderMap) -> String {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == COOKIE)
        .map(|(_, value)| value.trim().to_string())
        .unwrap_or_default()
}

/// Whether a request may change something: a browser's must come from a page Janus served itself
/// (its `Origin` names the same host and port the request went to). A client that is not a browser
/// sends neither header, and carries no cookie a page could have lent it.
fn same_origin(method: &Method, headers: &HeaderMap) -> bool {
    if matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS) {
        return true;
    }
    let host = headers.get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("");
    match headers.get(header::ORIGIN).and_then(|o| o.to_str().ok()) {
        Some(origin) => {
            let authority = origin.split_once("://").map(|(_, rest)| rest).unwrap_or("");
            !host.is_empty() && authority.eq_ignore_ascii_case(host)
        }
        None => headers
            .get("sec-fetch-site")
            .and_then(|s| s.to_str().ok())
            .is_none_or(|s| s == "same-origin" || s == "none"),
    }
}

/// Every request passes here first; see the module's comment for the rules.
// @project-doc docs/operations/20261031-security.md#paired_devices
pub(super) async fn guard(State((app, via)): State<(Arc<App>, Via)>, mut req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();
    if path.starts_with("/mcp/") {
        // The agents' endpoints have their own tokens and are reached only on this Mac.
        if via == Via::Lan {
            return StatusCode::NOT_FOUND.into_response();
        }
        return next.run(req).await;
    }
    let peer = req.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip());
    if via == Via::Lan {
        let why = if !app.lan_listening() {
            Some("手机访问已关闭")
        } else if !peer.is_some_and(|ip| crate::devices::network_of(ip).is_some()) {
            // The tailnet is the way in from outside; a public address is not let in.
            Some("手机访问只接受局域网和 Tailscale 里的设备")
        } else {
            None
        };
        if let Some(why) = why {
            let mut resp = ApiError::forbidden(why).into_response();
            resp.headers_mut().insert(header::CONNECTION, HeaderValue::from_static("close"));
            return resp;
        }
    }
    if !path.starts_with("/api/") {
        // The web client's own files: nothing in them is the person's.
        return next.run(req).await;
    }
    if via == Via::Local && peer.is_some_and(|ip| ip.is_loopback()) && from_local_app(req.headers()) {
        req.extensions_mut().insert(Client::App);
        return next.run(req).await;
    }
    if !same_origin(req.method(), req.headers()) {
        return ApiError::forbidden("拒绝了别的网站发来的请求").into_response();
    }
    if path == "/api/devices/claim" {
        return next.run(req).await;
    }
    let Some(device) = app.device_by_token(&cookie_token(req.headers())) else {
        return unauthorized("这台设备还没有配对：在 Mac 上打开 Janus 的「设置 › 手机访问」，扫码配对");
    };
    req.extensions_mut().insert(Client::Device(device.id.clone()));
    let resp = next.run(req).await;
    let streaming = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("text/event-stream"));
    if !streaming {
        return resp;
    }
    // An open stream ends when its device is unpaired, or when the network switch turns off.
    let (parts, body) = resp.into_parts();
    let stop = Box::pin(until_cut(app, device.id, via));
    Response::from_parts(parts, Body::from_stream(Cut { body: body.into_data_stream(), stop, done: false }))
}

/// Resolves once device `id` may no longer follow the stream it opened through `via`.
async fn until_cut(app: Arc<App>, id: String, via: Via) {
    let mut epoch = app.devices.epoch.subscribe();
    loop {
        if !app.device_paired(&id) || (via == Via::Lan && !app.lan_listening()) {
            return;
        }
        if epoch.changed().await.is_err() {
            return;
        }
    }
}

/// A response body that breaks off when `stop` resolves, so the connection closes.
struct Cut {
    body: axum::body::BodyDataStream,
    stop: Pin<Box<dyn Future<Output = ()> + Send>>,
    done: bool,
}

impl tokio_stream::Stream for Cut {
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.done {
            return Poll::Ready(None);
        }
        if self.stop.as_mut().poll(cx).is_ready() {
            self.done = true;
            return Poll::Ready(Some(Err(std::io::Error::other("这台设备的访问已结束"))));
        }
        Pin::new(&mut self.body).poll_next(cx).map(|item| item.map(|r| r.map_err(std::io::Error::other)))
    }
}

/// A new pairing code and where to open it, for the app's QR code.
pub(super) async fn pairing(State(app): State<Arc<App>>, Extension(client): Extension<Client>) -> ApiResult {
    local_app_only(&client, "配对码")?;
    reply(app.new_pairing()?)
}

/// Pair the device that opened a code: it gets its cookie.
pub(super) async fn claim(State(app): State<Arc<App>>, Json(body): Json<Value>) -> Response {
    match app.claim_device(&body) {
        Ok((device, token)) => {
            let cookie = format!("{COOKIE}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={COOKIE_MAX_AGE}");
            let mut resp = Json(json!({ "id": device.id, "name": device.name })).into_response();
            resp.headers_mut().insert(header::SET_COOKIE, HeaderValue::from_str(&cookie).expect("hex token"));
            resp
        }
        Err(ClaimRefused::Invalid(why)) => ApiError::bad_request(why).into_response(),
        Err(ClaimRefused::BadCode) => {
            unauthorized("配对码不对或已经过期：在 Mac 上的「设置 › 手机访问」重新显示二维码，再扫一次")
        }
    }
}

/// The device the request comes from.
pub(super) async fn me(State(app): State<Arc<App>>, Extension(client): Extension<Client>) -> Response {
    match client {
        Client::Device(id) => match app.devices().into_iter().find(|d| d.id == id) {
            Some(d) => Json(json!({ "id": d.id, "name": d.name })).into_response(),
            None => unauthorized("这台设备已经取消配对"),
        },
        Client::App => unauthorized("本机的 Janus 应用不是配对的设备"),
    }
}

pub(super) async fn list(State(app): State<Arc<App>>) -> ApiResult {
    reply(app.devices())
}

/// Unpair a device: the app may unpair any, a device only itself (its cookie is cleared too).
pub(super) async fn revoke(
    State(app): State<Arc<App>>,
    Extension(client): Extension<Client>,
    UrlPath(id): UrlPath<String>,
) -> ApiResult {
    if let Client::Device(me) = &client
        && *me != id
    {
        return Err(ApiError::forbidden("配对的设备只能取消自己的配对，别的设备在 Mac 上的 Janus 里取消"));
    }
    app.revoke_device(&id).map_err(|e| ApiError::not_found(format!("{e:#}")))?;
    let mut resp = ok()?;
    if client != Client::App {
        let gone = format!("{COOKIE}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0");
        resp.headers_mut().insert(header::SET_COOKIE, HeaderValue::from_str(&gone).expect("plain text"));
    }
    Ok(resp)
}

pub(super) async fn lan_access(State(app): State<Arc<App>>) -> ApiResult {
    reply(app.lan_access())
}

/// Turn the network switch: only the app on this Mac may, so a paired device cannot open Janus to
/// the network it is on. The listener starts or stops at once.
pub(super) async fn save_lan_access(
    State(app): State<Arc<App>>,
    Extension(client): Extension<Client>,
    Json(body): Json<Value>,
) -> ApiResult {
    local_app_only(&client, "手机访问的开关")?;
    app.save_lan_access(&body)?;
    super::lan::apply(&app);
    reply(app.lan_access())
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue, Method};

    fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(*k, HeaderValue::from_static(v));
        }
        h
    }

    /// A change goes through from a page Janus served, through the Vite proxy (which keeps the
    /// browser's `Host`) and from a client that is not a browser; not from another site's page.
    #[test]
    fn only_janus_pages_make_changes() {
        let ok = |pairs: &[(&'static str, &'static str)]| super::same_origin(&Method::POST, &headers(pairs));
        assert!(ok(&[("host", "192.168.1.5:7878"), ("origin", "http://192.168.1.5:7878")]));
        assert!(ok(&[("host", "localhost:5173"), ("origin", "http://localhost:5173")]), "the Vite proxy");
        assert!(ok(&[("host", "127.0.0.1:7878")]), "not a browser");
        assert!(!ok(&[("host", "127.0.0.1:7878"), ("origin", "http://evil.example")]));
        assert!(!ok(&[("host", "127.0.0.1:7878"), ("origin", "http://127.0.0.1:9999")]), "another port");
        assert!(!ok(&[("host", "127.0.0.1:7878"), ("origin", "null")]));
        assert!(!ok(&[("host", "127.0.0.1:7878"), ("sec-fetch-site", "cross-site")]));
        assert!(!ok(&[("origin", "http://127.0.0.1:7878")]), "no Host");
        assert!(
            super::same_origin(&Method::GET, &headers(&[("origin", "http://evil.example")])),
            "reads still need the cookie"
        );
    }

    #[test]
    fn the_cookie_is_found_among_others() {
        assert_eq!(super::cookie_token(&headers(&[("cookie", "a=1; janus_device=abc ; b=2")])), "abc");
        assert_eq!(super::cookie_token(&headers(&[("cookie", "janus_devicex=1")])), "");
    }
}
