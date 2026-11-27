//! Pairing and the device cookie, over HTTP as the clients make their requests: the app on this
//! Mac (no `Origin`, a loopback `Host`), a browser page (its `Origin` and `Sec-Fetch-Site`) and a
//! phone on the network listener.

use crate::http::{Via, router};
use crate::orchestrator::{App, test_app};
use reqwest::{Client as Http, StatusCode};
use serde_json::{Value, json};
use std::sync::Arc;

/// A fresh app, its routes served on a loopback port as `via`; and that port's base URL.
async fn served(name: &str, via: Via) -> (Arc<App>, String) {
    let dir = std::env::temp_dir().join(format!("janus-devices-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let app = test_app(&dir);
    let base = serve(&app, via).await;
    (app, base)
}

async fn serve(app: &Arc<App>, via: Via) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let service = router(app.clone(), via).into_make_service_with_connect_info::<std::net::SocketAddr>();
    tokio::spawn(async move {
        let _ = axum::serve(listener, service).await;
    });
    base
}

/// JSON in and out, as `reqwest` here is built without its `json` feature.
trait JsonBody {
    fn json_body(self, v: &Value) -> Self;
}

impl JsonBody for reqwest::RequestBuilder {
    fn json_body(self, v: &Value) -> Self {
        self.header("content-type", "application/json").body(v.to_string())
    }
}

trait ReadJson {
    async fn read(self) -> Value;
}

impl ReadJson for reqwest::Response {
    async fn read(self) -> Value {
        serde_json::from_str(&self.text().await.unwrap()).unwrap()
    }
}

fn http() -> Http {
    Http::builder().no_proxy().build().unwrap()
}

/// A page Janus served at `base` asks: with the `Origin` and `Sec-Fetch-Site` a browser adds.
fn page(req: reqwest::RequestBuilder, base: &str, cookie: Option<&str>) -> reqwest::RequestBuilder {
    let req = req.header("origin", base).header("sec-fetch-site", "same-origin");
    match cookie {
        Some(c) => req.header("cookie", format!("other=1; janus_device={c}")),
        None => req,
    }
}

/// A code the app asks for, as the app does.
async fn code(base: &str) -> String {
    let r: Value = http().post(format!("{base}/api/devices/pairing")).send().await.unwrap().read().await;
    r["code"].as_str().unwrap().to_string()
}

/// Claim `code` from a page; the status and the token its cookie carries.
async fn claim(base: &str, code: &str) -> (StatusCode, String) {
    let r = page(http().post(format!("{base}/api/devices/claim")), base, None)
        .json_body(&json!({ "code": code, "name": "iPhone" }))
        .send()
        .await
        .unwrap();
    let status = r.status();
    let cookie = r.headers().get("set-cookie").map(|v| v.to_str().unwrap().to_string()).unwrap_or_default();
    if status == StatusCode::OK {
        for part in ["HttpOnly", "SameSite=Strict", "Path=/", "Max-Age="] {
            assert!(cookie.contains(part), "{cookie}");
        }
    }
    let token = cookie.strip_prefix("janus_device=").and_then(|c| c.split(';').next()).unwrap_or("").to_string();
    (status, token)
}

async fn status(req: reqwest::RequestBuilder) -> StatusCode {
    req.send().await.unwrap().status()
}

/// Without its cookie a page reads nothing, the app on this Mac everything; a claimed code gives
/// the page a cookie that works, a made-up one does not.
#[tokio::test]
async fn a_browser_needs_its_device_cookie() {
    let (app, base) = served("cookie", Via::Local).await;
    let inbox = format!("{base}/api/inbox");
    assert_eq!(status(http().get(&inbox)).await, StatusCode::OK, "the app");
    assert_eq!(status(page(http().get(&inbox), &base, None)).await, StatusCode::UNAUTHORIZED);
    assert_eq!(
        status(http().get(&inbox).header("sec-fetch-site", "none")).await,
        StatusCode::UNAUTHORIZED,
        "a page opened by hand"
    );
    let r = page(http().get(&inbox), &base, Some("0123")).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED, "a made-up token");
    assert!(r.text().await.unwrap().contains("配对"), "it says how to pair");

    // A page cannot make a code for itself, even once paired.
    assert_eq!(
        status(page(http().post(format!("{base}/api/devices/pairing")), &base, None)).await,
        StatusCode::UNAUTHORIZED
    );
    let (s, token) = claim(&base, &code(&base).await).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        status(page(http().post(format!("{base}/api/devices/pairing")), &base, Some(&token))).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(status(page(http().get(&inbox), &base, Some(&token))).await, StatusCode::OK);
    let me: Value =
        page(http().get(format!("{base}/api/devices/me")), &base, Some(&token)).send().await.unwrap().read().await;
    assert_eq!(me["name"], "iPhone");
    let listed: Value = http().get(format!("{base}/api/devices")).send().await.unwrap().read().await;
    assert_eq!(listed[0]["id"], me["id"]);
    assert!(listed[0]["lastSeenAt"].as_i64().unwrap() > 0);
    // Only its hash is kept, in an owner-only file.
    let file = app.cfg.layout.device(me["id"].as_str().unwrap());
    let kept = std::fs::read_to_string(&file).unwrap();
    assert!(!kept.contains(&token), "{kept}");
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
    assert!(!std::fs::read(app.cfg.layout.db()).unwrap().windows(token.len()).any(|w| w == token.as_bytes()));
}

/// A code works once and not after it expires; wrong guesses void every open code.
#[tokio::test]
async fn codes_are_one_time_short_lived_and_not_guessable() {
    let (app, base) = served("codes", Via::Local).await;
    let c = code(&base).await;
    assert_eq!(c.len(), 10);
    assert_eq!(claim(&base, &c).await.0, StatusCode::OK);
    assert_eq!(claim(&base, &c).await.0, StatusCode::UNAUTHORIZED, "used");

    let old = code(&base).await;
    app.age_codes(crate::devices::CODE_TTL_MS + 1);
    assert_eq!(claim(&base, &old).await.0, StatusCode::UNAUTHORIZED, "expired");

    let good = code(&base).await;
    for _ in 0..crate::devices::MAX_FAILURES {
        assert_eq!(claim(&base, "aaaaaaaaaa").await.0, StatusCode::UNAUTHORIZED);
    }
    assert_eq!(claim(&base, &good).await.0, StatusCode::UNAUTHORIZED, "voided by the guesses");
    assert_eq!(claim(&base, &code(&base).await).await.0, StatusCode::OK, "a new code works");

    let r =
        page(http().post(format!("{base}/api/devices/claim")), &base, None).json_body(&json!({})).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
}

/// Another site's page is refused a change even with the cookie the browser would add, and so is
/// a page whose domain was pointed at this Mac (its `Origin` is its own domain, the cookie's is not).
#[tokio::test]
async fn another_site_cannot_make_changes() {
    let (_app, base) = served("csrf", Via::Local).await;
    let (_, token) = claim(&base, &code(&base).await).await;
    let marks = format!("{base}/api/inbox/marks");
    let from = |origin: &str| {
        http()
            .post(&marks)
            .header("origin", origin)
            .header("sec-fetch-site", "cross-site")
            .header("cookie", format!("janus_device={token}"))
            .json_body(&json!({ "key": "r1/x", "ignored": true }))
    };
    assert_eq!(status(from("https://evil.example")).await, StatusCode::FORBIDDEN);
    let r = http()
        .post(format!("{base}/api/devices/claim"))
        .header("origin", "https://evil.example")
        .json_body(&json!({ "code": code(&base).await, "name": "x" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN, "nor pair a device");
    // No Origin, but a browser's cross-site marker.
    let r = http()
        .post(&marks)
        .header("sec-fetch-site", "cross-site")
        .header("cookie", format!("janus_device={token}"))
        .json_body(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
}

/// Unpairing a device stops its cookie and ends its open stream; a device may unpair only itself.
#[tokio::test]
async fn an_unpaired_device_is_out_at_once() {
    let (_app, base) = served("revoke", Via::Local).await;
    let (_, a) = claim(&base, &code(&base).await).await;
    let (_, b) = claim(&base, &code(&base).await).await;
    let id_of = |t: String| {
        let base = base.clone();
        async move {
            let me: Value =
                page(http().get(format!("{base}/api/devices/me")), &base, Some(&t)).send().await.unwrap().read().await;
            me["id"].as_str().unwrap().to_string()
        }
    };
    let (id_a, id_b) = (id_of(a.clone()).await, id_of(b.clone()).await);
    assert_eq!(
        status(page(http().delete(format!("{base}/api/devices/{id_b}")), &base, Some(&a))).await,
        StatusCode::FORBIDDEN,
        "not another device"
    );

    let mut stream = page(http().get(format!("{base}/api/stream")), &base, Some(&a)).send().await.unwrap();
    assert_eq!(stream.status(), StatusCode::OK);
    assert!(stream.chunk().await.unwrap().is_some(), "ready");
    assert_eq!(status(http().delete(format!("{base}/api/devices/{id_a}"))).await, StatusCode::OK, "the app unpairs it");
    let ended = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Ok(Some(_)) = stream.chunk().await {}
    })
    .await;
    assert!(ended.is_ok(), "the open stream ends");
    assert_eq!(status(page(http().get(format!("{base}/api/inbox")), &base, Some(&a))).await, StatusCode::UNAUTHORIZED);

    // A device unpairs itself, and its cookie is cleared.
    let r = page(http().delete(format!("{base}/api/devices/{id_b}")), &base, Some(&b)).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    assert!(r.headers()["set-cookie"].to_str().unwrap().contains("Max-Age=0"));
    assert_eq!(status(page(http().get(format!("{base}/api/inbox")), &base, Some(&b))).await, StatusCode::UNAUTHORIZED);
    let listed: Value = http().get(format!("{base}/api/devices")).send().await.unwrap().read().await;
    assert_eq!(listed, json!([]));
}

/// On the network listener nothing passes as the app, not even a request that says it is from
/// this Mac; the agents' MCP endpoints are not there; what only the app may read stays with it.
#[tokio::test]
async fn the_network_listener_asks_everyone_for_a_cookie() {
    let (app, local) = served("lan", Via::Local).await;
    app.pretend_lan_listening();
    let lan = serve(&app, Via::Lan).await;
    let inbox = format!("{lan}/api/inbox");
    assert_eq!(status(http().get(&inbox).header("host", "127.0.0.1")).await, StatusCode::UNAUTHORIZED);
    assert_eq!(status(http().post(format!("{lan}/api/devices/pairing"))).await, StatusCode::UNAUTHORIZED);
    assert_eq!(status(http().post(format!("{lan}/mcp/r1/t/lead")).json_body(&json!({}))).await, StatusCode::NOT_FOUND);

    let (_, token) = claim(&lan, &code(&local).await).await;
    assert!(!token.is_empty(), "a phone pairs over the network");
    let phone = |req: reqwest::RequestBuilder| req.header("cookie", format!("janus_device={token}"));
    assert_eq!(status(phone(http().get(&inbox))).await, StatusCode::OK);
    for what in ["/api/library/mcp/m1/values", "/api/connections/c1/env", "/api/host-env"] {
        assert_eq!(status(phone(http().get(format!("{lan}{what}")))).await, StatusCode::FORBIDDEN, "{what}");
        assert_eq!(
            status(page(http().get(format!("{local}{what}")), &local, Some(&token))).await,
            StatusCode::FORBIDDEN,
            "{what} on this Mac"
        );
    }
    let r = phone(http().put(format!("{lan}/api/lan-access")))
        .json_body(&json!({ "enabled": false }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN, "a phone cannot turn the switch");

    // Off: the listener refuses what still reaches it, and its streams end.
    let mut stream = phone(http().get(format!("{lan}/api/stream"))).send().await.unwrap();
    assert!(stream.chunk().await.unwrap().is_some());
    app.devices.lan.lock().unwrap().stop = None;
    app.devices.epoch.send_modify(|e| *e += 1);
    let ended = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Ok(Some(_)) = stream.chunk().await {}
    })
    .await;
    assert!(ended.is_ok(), "the stream ends");
    assert_eq!(status(phone(http().get(&inbox))).await, StatusCode::FORBIDDEN);
}

/// The switch starts a listener on every interface beside the loopback one, on the same port, and
/// stops it again. (Linux refuses the wildcard beside a listening loopback socket; Janus runs on
/// macOS, which gives each connection to the most specific address bound.)
#[cfg(target_os = "macos")]
#[tokio::test]
async fn the_switch_opens_and_closes_the_network_listener() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let dir = std::env::temp_dir().join(format!("janus-devices-switch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let app = crate::orchestrator::test_app_with(&dir, |cfg| cfg.addr = addr.to_string());
    let service = router(app.clone(), Via::Local).into_make_service_with_connect_info::<std::net::SocketAddr>();
    tokio::spawn(async move {
        let _ = axum::serve(listener, service).await;
    });
    let base = format!("http://{addr}");
    let off: Value = http().get(format!("{base}/api/lan-access")).send().await.unwrap().read().await;
    assert_eq!(off["enabled"], false);
    let on: Value = http()
        .put(format!("{base}/api/lan-access"))
        .json_body(&json!({ "enabled": true }))
        .send()
        .await
        .unwrap()
        .read()
        .await;
    assert_eq!(on["listening"], true, "{on}");
    assert_eq!(on["urls"].as_array().unwrap().last().unwrap(), &json!(base));
    // The app on this Mac still reaches the loopback listener as before.
    assert_eq!(status(http().get(format!("{base}/api/inbox"))).await, StatusCode::OK);
    // The network side of the port is taken: by Janus. (Connecting to it from here would wait on
    // the macOS firewall, which asks before a new binary accepts connections from the network.)
    let wildcard = || std::net::TcpListener::bind(("0.0.0.0", addr.port()));
    assert!(wildcard().is_err(), "Janus listens on every interface");
    http().put(format!("{base}/api/lan-access")).json_body(&json!({ "enabled": false })).send().await.unwrap();
    let freed = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while wildcard().is_err() {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await;
    assert!(freed.is_ok(), "the network listener is gone");
    let off: Value = http().get(format!("{base}/api/lan-access")).send().await.unwrap().read().await;
    assert_eq!(off["listening"], false);
}
