//! The web client's pages: every path that is not the API's or the agents' is a file of the
//! client embedded at build time (`build.rs`), or the client's page itself, whose router shows
//! what the path names. Files under `assets/` have their content's hash in their name and are kept
//! by the browser for good; the page is asked for again each time, so a new build shows at once.
// @project-doc docs/architecture/20261126-web.md#serving

use axum::http::{HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/web_assets.rs"));
}

/// What the service shows when it was built without the web client.
const NOT_BUILT: &str = "<!doctype html><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>Janus</title>\
<body style=\"font: 15px/1.5 -apple-system, sans-serif; max-width: 34em; margin: 4em auto; padding: 0 1em\">\
<h1 style=\"font-size: 1.3em\">Janus Web 版没有构建</h1><p>在仓库里运行 <code>just web</code>，再重新构建 Janus 服务。</p>\
<p>The web version is not built. Run <code>just web</code> in the repository, then build the Janus service again.</p></body>";

fn content_type(name: &str) -> &'static str {
    match name.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        "webmanifest" => "application/manifest+json",
        _ => "application/octet-stream",
    }
}

fn file(name: &str) -> Option<&'static [u8]> {
    embedded::FILES.iter().find(|(n, _)| *n == name).map(|(_, bytes)| *bytes)
}

fn page(status: StatusCode, name: &str, body: &'static [u8], cache: &'static str) -> Response {
    let mut res = (status, body).into_response();
    let h = res.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type(name)));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    h.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    res
}

/// The answer for a path no route took: an API path is not there (404); any other is a file of the
/// web client, or its page.
pub(super) async fn serve(uri: Uri) -> Response {
    let path = uri.path();
    if path == "/api" || path.starts_with("/api/") || path.starts_with("/mcp/") {
        return (StatusCode::NOT_FOUND, "没有这个接口").into_response();
    }
    let name = path.trim_start_matches('/');
    if let Some(bytes) = file(name) {
        let cache = if name.starts_with("assets/") { "public, max-age=31536000, immutable" } else { "no-cache" };
        return page(StatusCode::OK, name, bytes, cache);
    }
    // A missing file of the client's own (a stale hashed name) is not the page.
    if name.starts_with("assets/") {
        return (StatusCode::NOT_FOUND, "没有这个文件").into_response();
    }
    match file("index.html") {
        Some(index) => page(StatusCode::OK, "index.html", index, "no-cache"),
        None => page(StatusCode::OK, "index.html", NOT_BUILT.as_bytes(), "no-cache"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    async fn get(path: &str) -> (StatusCode, String, String) {
        let res = serve(path.parse().unwrap()).await;
        let status = res.status();
        let kind = res.headers().get(header::CONTENT_TYPE).map(|v| v.to_str().unwrap().to_string()).unwrap_or_default();
        let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        (status, kind, String::from_utf8_lossy(&body).into_owned())
    }

    /// Every path but the API's is the client: a file when it is one, else its page (which routes
    /// by the path itself), and an unknown API path stays a 404 rather than becoming the page.
    #[tokio::test]
    async fn paths_outside_the_api_are_the_web_client() {
        let (status, kind, body) = get("/runs/r1").await;
        assert_eq!(status, StatusCode::OK);
        assert!(kind.starts_with("text/html"), "{kind}");
        assert!(body.contains("<html") || body.contains("<!doctype html"), "{body}");
        assert_eq!(get("/api/nothing").await.0, StatusCode::NOT_FOUND);
        assert_eq!(get("/mcp/x").await.0, StatusCode::NOT_FOUND);
        assert_eq!(get("/assets/gone-1234.js").await.0, StatusCode::NOT_FOUND);
        for (name, _) in embedded::FILES {
            let (status, kind, _) = get(&format!("/{name}")).await;
            assert_eq!(status, StatusCode::OK, "{name}");
            assert_eq!(kind, content_type(name), "{name}");
        }
    }
}
