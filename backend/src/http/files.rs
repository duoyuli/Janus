//! The files a client shows for a run, read through the service so that a client without this
//! Mac's disk (the Web app) shows the same: the run's Markdown documents and what they link to,
//! the images pasted into it, a task's diff file by file, and an adapter's install log. Each route
//! serves only what the run names (`run_view::docs`) or Janus stored for it; the rest of the disk,
//! and of Janus home, is refused (docs/operations/20261031-security.md#run_files).
// @project-doc docs/interfaces/20261031-http_api.md#run_files_read

use super::setup::PathQuery;
use super::{ApiError, ApiResult, reply};
use crate::db::RunRow;
use crate::orchestrator::App;
use axum::Json;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::header;
use axum::response::IntoResponse;
use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn stored_run(app: &App, id: &str) -> Result<RunRow, ApiError> {
    app.db.run(id)?.ok_or_else(|| ApiError::not_found("任务不存在"))
}

/// `path` with links resolved, when it is a file.
fn real_file(path: &Path) -> Result<PathBuf, ApiError> {
    match path.canonicalize() {
        Ok(p) if p.is_file() => Ok(p),
        _ => Err(ApiError::not_found("文件不存在")),
    }
}

/// `real` is inside `dir` (both with links resolved).
fn inside(real: &Path, dir: &Path) -> bool {
    dir.canonicalize().is_ok_and(|d| real.starts_with(d))
}

/// A file Janus stored for run `row` itself: an artifact or a diff of its tasks (`artifacts`), or
/// an image pasted into it (`attachment`).
fn stored_for(app: &App, row: &RunRow, real: &Path, attachment: bool) -> bool {
    let layout = &app.cfg.layout;
    let ok = if attachment { layout.is_attachment(real) } else { layout.is_artifact(real) };
    ok && inside(real, &layout.run(row.kind, &row.id).data)
}

/// The document `path` of run `row`, when the client may read it: one of the run's Markdown
/// documents (`RunView::docs`), or one of its artifacts.
fn run_doc(app: &App, row: &RunRow, path: &str) -> Result<PathBuf, ApiError> {
    let real = real_file(Path::new(path))?;
    if stored_for(app, row, &real, false) {
        return Ok(real);
    }
    let work = app.cfg.layout.run(row.kind, &row.id).work;
    let docs = app.with_fold(&row.id, None, |f| f.docs(&work))?.unwrap_or_default();
    let listed = docs.iter().any(|d| Path::new(&d.path).canonicalize().is_ok_and(|p| p == real));
    if !listed {
        return Err(ApiError::forbidden("只能读取这个任务交付、保存或提到的文档"));
    }
    own_in_home(app, row, &real)?;
    Ok(real)
}

/// In Janus home only what is the run's own: its working copy and what Janus stored for it.
fn own_in_home(app: &App, row: &RunRow, real: &Path) -> Result<(), ApiError> {
    let layout = &app.cfg.layout;
    let own = inside(real, &layout.run(row.kind, &row.id).work)
        || stored_for(app, row, real, false)
        || stored_for(app, row, real, true);
    if inside(real, layout.home()) && !own {
        return Err(ApiError::forbidden("Janus home 里只能读这个任务自己的文件"));
    }
    Ok(())
}

fn text_reply(path: &Path) -> ApiResult {
    let text = std::fs::read_to_string(path).map_err(|_| ApiError::bad_request("这个文件不是文本"))?;
    Ok(([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], text).into_response())
}

/// One of the run's Markdown documents or artifacts, as text.
// @project-doc docs/interfaces/20261031-http_api.md#run_files_read
pub(super) async fn doc(
    State(app): State<Arc<App>>,
    UrlPath(id): UrlPath<String>,
    Query(q): Query<PathQuery>,
) -> ApiResult {
    let row = stored_run(&app, &id)?;
    text_reply(&run_doc(&app, &row, &q.path)?)
}

#[derive(Deserialize)]
pub(super) struct AssetQuery {
    doc: String,
    /// The file a link of the document leads to, resolved from the document's folder.
    path: String,
}

/// An image type a document shows, by its name.
fn image_type(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        _ => return None,
    })
}

/// `%20` and the like in a link, decoded.
fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or(""), 16)
        {
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The link targets written in a Markdown text: `[a](target)`, `![a](target "title")`,
/// `[a](<target>)`, `[id]: target` and `<img src="target">`.
pub(crate) fn link_targets(text: &str) -> Vec<String> {
    let mut out = vec![];
    let mut rest = text;
    while let Some(i) = rest.find("](") {
        let after = &rest[i + 2..];
        let t = match after.strip_prefix('<') {
            Some(a) => a.split('>').next().unwrap_or(""),
            None => after.split(|c: char| c == ')' || c.is_whitespace()).next().unwrap_or(""),
        };
        out.push(t.to_string());
        rest = after;
    }
    for line in text.lines() {
        let l = line.trim_start();
        if l.starts_with('[')
            && let Some((_, t)) = l.split_once("]:")
        {
            out.extend(t.split_whitespace().next().map(|t| t.trim_matches(['<', '>']).to_string()));
        }
    }
    let mut rest = text;
    while let Some(i) = rest.find("src=") {
        let after = &rest[i + 4..];
        let q = after.chars().next().filter(|c| *c == '"' || *c == '\'');
        let t = match q {
            Some(q) => after[1..].split(q).next().unwrap_or(""),
            None => after.split(|c: char| c == '>' || c.is_whitespace()).next().unwrap_or(""),
        };
        out.push(t.to_string());
        rest = after;
    }
    out.retain(|t| !t.is_empty());
    out
}

/// Where link `target` of document `doc` leads: from the document's folder, `~` from the home
/// directory, `%20` and the like decoded, without a `#heading` or `?query`.
fn resolve(doc: &Path, target: &str) -> PathBuf {
    let t = decode(target.split(['#', '?']).next().unwrap_or(""));
    if let Some(rest) = t.strip_prefix("~/") {
        PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(rest)
    } else if t.starts_with('/') {
        PathBuf::from(&t)
    } else {
        doc.parent().unwrap_or(Path::new("/")).join(&t)
    }
}

/// What a document links to or shows (`path`, resolved): an image, or another Markdown file.
/// Only what one of the document's links leads to; a Markdown file as text, an image as itself.
/// Nothing in Janus home but the run's own working copy and stored files.
// @project-doc docs/interfaces/20261031-http_api.md#run_files_read
pub(super) async fn doc_asset(
    State(app): State<Arc<App>>,
    UrlPath(id): UrlPath<String>,
    Query(q): Query<AssetQuery>,
) -> ApiResult {
    let row = stored_run(&app, &id)?;
    let doc = run_doc(&app, &row, &q.doc)?;
    let real = real_file(Path::new(&q.path))?;
    let source = std::fs::read_to_string(&doc).unwrap_or_default();
    let linked = link_targets(&source).iter().any(|t| resolve(&doc, t).canonicalize().is_ok_and(|p| p == real));
    if !linked {
        return Err(ApiError::forbidden("文档里没有链到这个文件"));
    }
    own_in_home(&app, &row, &real)?;
    if let Some(mime) = image_type(&real) {
        let bytes = std::fs::read(&real).map_err(anyhow::Error::from)?;
        return Ok(([(header::CONTENT_TYPE, mime)], bytes).into_response());
    }
    if crate::run_view::docs::is_markdown(&real.to_string_lossy()) {
        return text_reply(&real);
    }
    Err(ApiError::forbidden("文档里只能打开图片和 Markdown 文件"))
}

/// An image pasted into the run's goal or one of its messages.
// @project-doc docs/interfaces/20261031-http_api.md#run_files_read
pub(super) async fn image(
    State(app): State<Arc<App>>,
    UrlPath(id): UrlPath<String>,
    Query(q): Query<PathQuery>,
) -> ApiResult {
    let row = stored_run(&app, &id)?;
    let real = real_file(Path::new(&q.path))?;
    if !stored_for(&app, &row, &real, true) {
        return Err(ApiError::forbidden("只能读取这个任务里贴的图片"));
    }
    let bytes = std::fs::read(&real).map_err(anyhow::Error::from)?;
    let mime = crate::attachments::sniff(&bytes).map_or("application/octet-stream", |(m, _)| m);
    Ok(([(header::CONTENT_TYPE, mime)], bytes).into_response())
}

/// A diff of the run (one task's, or the whole run's), file by file (`diff::parse`).
// @project-doc docs/interfaces/20261031-http_api.md#run_files_read
pub(super) async fn diff(
    State(app): State<Arc<App>>,
    UrlPath(id): UrlPath<String>,
    Query(q): Query<PathQuery>,
) -> ApiResult {
    let row = stored_run(&app, &id)?;
    let real = real_file(Path::new(&q.path))?;
    if !stored_for(&app, &row, &real, false) {
        return Err(ApiError::forbidden("只能读取这个任务的 diff"));
    }
    let text = std::fs::read_to_string(&real).map_err(|_| ApiError::bad_request("这个 diff 不是文本"))?;
    reply(json!({ "files": crate::diff::parse(&text) }))
}

#[derive(Deserialize)]
pub(super) struct ReviewBody {
    path: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    comments: Vec<crate::diff::Comment>,
}

/// The comments of a review of one of the run's diffs, written as one message to an agent in the
/// run's language (`diff::message`); the client sends it as it sends any message.
// @project-doc docs/interfaces/20261031-http_api.md#run_files_read
pub(super) async fn review_message(
    State(app): State<Arc<App>>,
    UrlPath(id): UrlPath<String>,
    Json(b): Json<ReviewBody>,
) -> ApiResult {
    let row = stored_run(&app, &id)?;
    let real = real_file(Path::new(&b.path))?;
    if !stored_for(&app, &row, &real, false) {
        return Err(ApiError::forbidden("只能审阅这个任务的 diff"));
    }
    let artifact = app
        .with_fold(&id, None, |f| f.state.artifacts.iter().find(|a| a.path == b.path).cloned())?
        .flatten()
        .ok_or_else(|| ApiError::not_found("这个任务没有这个 diff"))?;
    let text = std::fs::read_to_string(&real).map_err(|_| ApiError::bad_request("这个 diff 不是文本"))?;
    let files = crate::diff::parse(&text);
    let subject = (!artifact.task.is_empty()).then_some(artifact.task.as_str());
    let lang = crate::lang::Lang::of_config(&row.config);
    reply(json!({ "text": crate::diff::message(lang, subject, &artifact.name, &files, &b.comments, &b.summary) }))
}

/// What installing and updating one adapter printed (`logs/adapters/<id>.log`).
// @project-doc docs/interfaces/20261031-http_api.md#run_files_read
pub(super) async fn adapter_log(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> ApiResult {
    if !crate::adapters::ADAPTERS.iter().any(|a| a.id == id) {
        return Err(ApiError::not_found(format!("没有这个运行组件：{id}")));
    }
    let path = app.cfg.layout.adapter_log(&id);
    if !path.is_file() {
        return Err(ApiError::not_found("还没有安装记录"));
    }
    text_reply(&path)
}
