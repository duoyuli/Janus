//! What a client reads about a run's conversations and files through the service: the
//! conversation in turns with what sits beside it, the run's diff and documents, the images
//! pasted into it, and the live previews on its event stream. A client without this Mac's disk
//! reads the same, and nothing outside what the run names.

use super::*;
use crate::lang::Lang;
use crate::run_view::Layout;

async fn get(app: &App, path: &str) -> (u16, String) {
    let r = reqwest::get(format!("http://{}{path}", app.cfg.addr)).await.unwrap();
    (r.status().as_u16(), r.text().await.unwrap())
}

fn q(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// A finished task's conversation comes in turns with its plan, commands and edits; its diff
/// reads file by file; and only the run's own files are served.
#[tokio::test(flavor = "multi_thread")]
async fn a_run_serves_its_conversations_and_only_its_files() {
    let h = harness("files", &[("lead", false), ("m1", true)], json!({ "T1": ["rich"] }));
    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T1 done", |e| task_event(e, "task.succeeded", "T1")).await;
    wait_until(&h.app, &run, "T1's diff", |e| e.iter().any(|e| e.kind == "artifact.created")).await;

    let view = h.app.run_view(&run, None, Lang::Zh, Layout::default()).unwrap().unwrap();
    let s = &view.sessions["task:T1"];
    assert_eq!(s.plan.len(), 2);
    assert!(s.commands.iter().any(|c| c.name == "review" && c.hint == "要审查什么"), "{:?}", s.commands);
    let v = serde_json::to_value(&view).unwrap();
    let rows = v["sessions"]["task:T1"]["rows"].as_array().unwrap();
    assert!(rows.iter().any(|r| r["kind"] == "head"), "{rows:?}");
    let edits = rows.iter().find(|r| r["kind"] == "edits").expect("the turn's edits");
    let change = &edits["files"][0]["changes"][0];
    let lines = &v["diffs"][change["seq"].to_string()][change["index"].as_u64().unwrap() as usize]["lines"];
    assert!(lines.as_array().unwrap().iter().any(|l| l["mark"] == "+" && l["text"] == "sdk()"), "{lines}");

    // The suggestions for a draft come from the same conversation.
    let (code, body) = get(&h.app, &format!("/api/runs/{run}/suggest?key=task:T1&draft={}", q("/rev"))).await;
    assert_eq!(code, 200, "{body}");
    let s: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(s["rows"][0]["completion"], "/review ");

    let diff = view.state.artifacts.iter().find(|a| a.kind.ends_with("diff")).expect("a diff").path.clone();
    let (code, body) = get(&h.app, &format!("/api/runs/{run}/diff?path={}", q(&diff))).await;
    assert_eq!(code, 200, "{body}");
    let files: Value = serde_json::from_str(&body).unwrap();
    assert!(files["files"].as_array().unwrap().iter().any(|f| f["path"] == "pay.js"), "{files}");
    let (code, body) = get(&h.app, &format!("/api/runs/{run}/doc?path={}", q(&diff))).await;
    assert_eq!(code, 200, "an artifact reads as a document: {body}");

    // Nothing else: a file of the workspace no event names, the database, a file elsewhere, and
    // a run's file asked for as another run's.
    let db = h.app.cfg.layout.db().to_string_lossy().into_owned();
    let readme = h.repo.join("README.md").to_string_lossy().into_owned();
    for path in [&readme, &db, &"/etc/hosts".to_string()] {
        let (code, _) = get(&h.app, &format!("/api/runs/{run}/doc?path={}", q(path))).await;
        assert_eq!(code, 403, "{path}");
        let (code, _) = get(&h.app, &format!("/api/runs/{run}/image?path={}", q(path))).await;
        assert_eq!(code, 403, "{path}");
        let (code, _) = get(&h.app, &format!("/api/runs/{run}/diff?path={}", q(path))).await;
        assert_eq!(code, 403, "{path}");
    }
    let (code, _) = get(&h.app, &format!("/api/runs/nope/diff?path={}", q(&diff))).await;
    assert_eq!(code, 404);
    let (code, _) = get(&h.app, "/api/adapters/..%2F..%2Fjanus/log").await;
    assert_eq!(code, 404, "an adapter Janus does not run has no log");

    // A document someone mentions is the run's to read, and so is what it links to; nothing else.
    // In the run's own working copy, as a goal on a Git repository works.
    let work = PathBuf::from(&view.state.run.cwd).canonicalize().unwrap();
    std::fs::write(work.join("notes.md"), "# 笔记\n\n![截图](shot.png)\n\n见 [另一份](other%20doc.md#细节)\n").unwrap();
    std::fs::write(work.join("shot.png"), b"\x89PNG\r\n\x1a\nrest").unwrap();
    std::fs::write(work.join("other doc.md"), "另一份").unwrap();
    let notes = work.join("notes.md").to_string_lossy().into_owned();
    act(&h, &run, json!({ "type": "message", "agent": "lead", "text": format!("看看 {notes}") }));
    wait_until(&h.app, &run, "the message", |e| has(e, "user.message")).await;
    let view = h.app.run_view(&run, None, Lang::Zh, Layout::default()).unwrap().unwrap();
    assert!(view.docs.iter().any(|d| d.path == notes), "{:?}", view.docs);
    let (code, body) = get(&h.app, &format!("/api/runs/{run}/doc?path={}", q(&notes))).await;
    assert!(code == 200 && body.starts_with("# 笔记"), "{code} {body}");
    let asset = |p: &Path| format!("/api/runs/{run}/doc/asset?doc={}&path={}", q(&notes), q(&p.to_string_lossy()));
    assert_eq!(get(&h.app, &asset(&work.join("shot.png"))).await.0, 200, "a picture it shows");
    assert_eq!(get(&h.app, &asset(&work.join("other doc.md"))).await.1, "另一份", "a document it links to");
    assert_eq!(get(&h.app, &asset(&work.join("README.md"))).await.0, 403, "a file it does not link to");

    // A review of the diff comes back as one message in the run's language.
    let line = files["files"][0]["hunks"][0]["lines"][0]["id"].clone();
    let r = reqwest::Client::new()
        .post(format!("http://{}/api/runs/{run}/diff/message", h.app.cfg.addr))
        .header("content-type", "application/json")
        .body(
            json!({ "path": diff, "summary": "整体可以", "comments": [{ "line": line, "text": "看一下" }] })
                .to_string(),
        )
        .send()
        .await
        .unwrap();
    let body: Value = serde_json::from_str(&r.text().await.unwrap()).unwrap();
    let text = body["text"].as_str().unwrap().to_string();
    assert!(text.starts_with("对T1 的改动（") && text.contains("看一下"), "{text}");
    finish_lead(&h);
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// The run's stream sends each conversation's whole preview as it changes, and null once the
/// record holds what it previewed.
#[tokio::test(flavor = "multi_thread")]
async fn the_stream_sends_each_conversations_preview() {
    let h = harness("previews", &[("lead", false), ("m1", true)], json!({ "T1": ["done"] }));
    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T1 done", |e| task_event(e, "task.succeeded", "T1")).await;
    let last = events(&h.app, &run).last().unwrap().seq;
    let mut stream =
        reqwest::get(format!("http://{}/api/runs/{run}/stream?after={last}", h.app.cfg.addr)).await.unwrap();
    // The stream is open once its first keep-alive or event could come; give it a moment.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let frame = json!({ "kind": "say", "actor": "m1", "task": "T1", "attempt": 1, "seq": last, "text": "正在写", "thought": "" });
    h.app.live.send(crate::orchestrator::live::Live { run: run.clone(), data: frame }).unwrap();
    let mut seen = String::new();
    while !seen.contains("正在写") {
        let chunk = tokio::time::timeout(Duration::from_secs(5), stream.chunk()).await.unwrap().unwrap().unwrap();
        seen.push_str(&String::from_utf8_lossy(&chunk));
    }
    let data = seen.lines().find(|l| l.starts_with("data:") && l.contains("previews")).unwrap();
    let f: Value = serde_json::from_str(data.trim_start_matches("data:").trim()).unwrap();
    assert_eq!(f["previews"]["task:T1"]["text"], "正在写");
    assert_eq!(f["previews"]["agent:m1"]["text"], "正在写");
    finish_lead(&h);
    let _ = std::fs::remove_dir_all(&h.dir);
}

/// The global stream carrying one run (`?run=`) sends that run's events and previews as the run's
/// own stream does, beside its notices: a web page keeps one stream, not two, within a browser's
/// six connections to the host.
#[tokio::test(flavor = "multi_thread")]
async fn the_global_stream_carries_one_runs_events_and_previews() {
    let h = harness("one-stream", &[("lead", false), ("m1", true)], json!({ "T1": ["done"] }));
    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T1 done", |e| task_event(e, "task.succeeded", "T1")).await;
    let last = events(&h.app, &run).last().unwrap().seq;
    let mut stream = reqwest::get(format!("http://{}/api/stream?run={run}&after={}&lang=zh", h.app.cfg.addr, last - 1))
        .await
        .unwrap();
    assert_eq!(stream.status(), 200);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let frame = json!({ "kind": "say", "actor": "m1", "task": "T1", "attempt": 1, "seq": last, "text": "一条流里", "thought": "" });
    h.app.live.send(crate::orchestrator::live::Live { run: run.clone(), data: frame }).unwrap();
    let mut seen = String::new();
    while !seen.contains("一条流里") {
        let chunk = tokio::time::timeout(Duration::from_secs(5), stream.chunk()).await.unwrap().unwrap().unwrap();
        seen.push_str(&String::from_utf8_lossy(&chunk));
    }
    assert!(seen.contains("event: ready"), "{seen}");
    // The backlog after `after`: the run's last event, unnamed, whole.
    let backlog = seen
        .lines()
        .filter(|l| l.starts_with("data:"))
        .filter_map(|l| serde_json::from_str::<Value>(l.trim_start_matches("data:").trim()).ok())
        .find(|v| v["seq"] == last && v["run_id"] == run.as_str());
    assert!(backlog.is_some(), "{seen}");
    assert!(seen.contains("event: live"), "{seen}");
    // A run that is not followed sends nothing of its own.
    let plain = reqwest::get(format!("http://{}/api/stream?lang=xx&run={run}", h.app.cfg.addr)).await.unwrap();
    assert_eq!(plain.status(), 400);
    finish_lead(&h);
    let _ = std::fs::remove_dir_all(&h.dir);
}
