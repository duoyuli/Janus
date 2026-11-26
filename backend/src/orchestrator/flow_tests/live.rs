//! Live frames: what a session is thinking and running shows before it is on record, and the
//! record itself does not change.

use super::*;

/// A running task previews its thinking and its running call, then ends its preview; the frames
/// are never stored, and the events are the ones a run without a viewer writes.
#[tokio::test(flavor = "multi_thread")]
async fn a_session_previews_what_it_is_doing() {
    let h = harness("live", &[("lead", false), ("m1", true)], json!({ "T1": ["rich"] }));
    let mut frames = h.app.live.subscribe();
    let run = start(&h, json!([task("T1", "m1", json!({}))])).await;
    wait_until(&h.app, &run, "T1 done", |e| task_event(e, "task.succeeded", "T1")).await;
    let mut seen = Vec::new();
    while let Ok(f) = frames.try_recv() {
        if f.run == run && f.data["task"] == "T1" {
            seen.push(f.data);
        }
    }
    // The thought goes on record at the first call; a frame before that previews what came so far.
    for f in seen.iter().filter(|f| f["kind"] == "say") {
        assert!("先看看 pay.js 再改。".starts_with(f["thought"].as_str().unwrap()) && f["actor"] == "m1", "{f}");
    }
    let tool = seen.iter().find(|f| f["kind"] == "tool").expect("a preview of the running call");
    assert_eq!(
        (tool["callId"].clone(), tool["title"].clone(), tool["status"].clone()),
        (json!("rich-1"), json!("Edit pay.js"), json!("in_progress"))
    );
    assert!(seen.iter().any(|f| f["kind"] == "end"), "the turn's preview ends");
    let seq = tool["seq"].as_i64().unwrap();
    let evs = events(&h.app, &run);
    let started = evs.iter().find(|e| e.kind.as_str() == "tool.started").unwrap().seq;
    assert!(seq >= started && evs.iter().any(|e| e.seq == seq), "a frame names the newest event it follows");
    assert!(evs.iter().all(|e| !e.kind.as_str().starts_with("live")), "frames are not stored");
    assert!(
        evs.iter()
            .any(|e| e.kind.as_str() == "agent.thought" && e.data.as_ref().unwrap()["text"] == "先看看 pay.js 再改。"),
        "the thought is on record as before"
    );
    finish_lead(&h);
    let _ = std::fs::remove_dir_all(&h.dir);
}
