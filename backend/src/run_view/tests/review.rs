//! Reviewing a delivery: where the run stands after one, and what the review shows first.

use super::*;
use crate::run_view::review::{ReviewStage, SignalKind};

const IDLE: &str = r#"{ "phase": "waiting_for_input", "turn": "idle" }"#;

fn idle() -> Value {
    serde_json::from_str(IDLE).unwrap()
}

fn start() -> Log {
    let mut log = Log::default();
    log.add(ev("run.created").actor("user").data(json!({ "mode": "goal", "goal": "上线支付页" })));
    log
}

/// The person writes to the lead after a delivery: the message, then its routing as a prompt.
pub(super) fn ask(log: &mut Log, text: &str) {
    let id = format!("m{}", log.events.len() + 1);
    log.add(ev("user.message").actor("user").to("lead").data(json!({ "id": id, "text": text })));
    log.add(ev("message.routed").to("lead").data(json!({ "id": id, "delivery": "prompt" })));
}

pub(super) fn delivered(log: &mut Log, d: Value) {
    log.add(ev("run.delivered").data(d));
}

fn done() -> Value {
    json!({ "headline": "做完了", "summary": "改了 a.rs" })
}

fn stage(log: &Log, live: bool) -> Option<ReviewStage> {
    log.view_in(Lang::Zh, live, &json!({})).review.map(|r| r.stage)
}

#[test]
fn a_delivery_is_reviewed_only_while_it_waits_for_the_person() {
    let mut log = start();
    assert_eq!(stage(&log, true), None, "nothing to review before a delivery");
    delivered(&mut log, done());
    assert_eq!(stage(&log, true), Some(ReviewStage::Awaiting));
    // The person asks for more: the lead works on it, and the delivery waits.
    ask(&mut log, "顺便改一下文案");
    assert_eq!(stage(&log, true), Some(ReviewStage::LeadWorking));
    assert_eq!(stage(&log, false), Some(ReviewStage::Awaiting), "no lead session is there after a restart");
    // It answered without changing anything: the same delivery waits again.
    let mut answered = log.clone();
    answered.add(ev("agent.status").actor("lead").data(idle()));
    assert_eq!(stage(&answered, true), Some(ReviewStage::Awaiting));
    // It changed something: a second delivery, of this round.
    delivered(&mut log, done());
    let v = log.view();
    let r = v.state.result().unwrap();
    assert!(r.n == 2 && r.since == 2 && v.state.deliveries.len() == 2);
    let review = v.review.unwrap();
    assert_eq!(review.stage, ReviewStage::Awaiting);
    assert_eq!(review.requests.iter().map(|a| a.text.as_str()).collect::<Vec<_>>(), ["顺便改一下文案"]);
    assert!(v.state.round_requests(&v.state.deliveries[0]).is_empty(), "the first delivery answers the goal");
    log.add(ev("risk.accepted").actor("user"));
    assert_eq!(stage(&log, true), Some(ReviewStage::Accepted));
}

#[test]
fn after_a_delivery_the_page_says_who_it_waits_on() {
    let mut first = start();
    delivered(&mut first, done());
    ask(&mut first, "再查一遍");
    // The person interrupts the lead and holds it, until they hand it back.
    let mut held = first.clone();
    held.add(ev("session.interrupted").actor("user").to("lead"));
    held.add(ev("agent.status").actor("lead").data(idle()));
    assert_eq!(stage(&held, true), Some(ReviewStage::Held));
    held.add(ev("control.released").actor("user").to("lead"));
    assert_eq!(stage(&held, true), Some(ReviewStage::LeadWorking));
    // The lead hands out subtasks: the team works, the run is running again.
    let mut team = first.clone();
    team.add(ev("orchestration.accepted").actor("lead").data(json!({ "tasks": [
        { "id": "T2", "title": "改", "agent": "m1", "deps": [], "accept": ["好"] },
    ]})));
    assert_eq!(stage(&team, true), Some(ReviewStage::TeamWorking));
    team.add(ev("attempt.started").task("T2"));
    team.add(ev("approval.requested").actor("m1").task("T2").data(json!({ "key": "a-1", "command": "rm" })));
    assert_eq!(stage(&team, true), Some(ReviewStage::Asking));
    // The merge: the lead resolves a conflict; then it is the person's call.
    let merge = |state: &str| {
        let mut log = start();
        delivered(&mut log, json!({ "merge": { "state": state, "note": "冲突" } }));
        log.view().review.map(|r| (r.stage, r.conflict))
    };
    assert_eq!(merge("resolving"), Some((ReviewStage::Resolving, false)));
    assert_eq!(merge("conflict"), Some((ReviewStage::Blocked, true)));
    assert_eq!(merge("blocked"), Some((ReviewStage::Blocked, false)));
    // Stopped after its delivery.
    let mut stopped = start();
    delivered(&mut stopped, done());
    stopped.add(ev("run.interrupted"));
    assert_eq!(stage(&stopped, true), Some(ReviewStage::Stopped));
}

/// A chat has no review.
#[test]
fn a_chat_has_no_review() {
    let mut log = super::chat_opened();
    delivered(&mut log, done());
    assert!(log.view().review.is_none());
}

/// The lead's questions come first, then what Janus found; the executors' own risks stay with
/// their tasks.
#[test]
fn what_needs_looking_at_is_the_leads_questions_then_januss_findings() {
    let mut log = start();
    delivered(
        &mut log,
        json!({
            "decisions": ["被忽略的文件要带回吗？"],
            "risks": [{ "task": "T3", "text": "T3：失败，没有交付结果" }],
            "leadEdits": ["a.rs", "b.rs"],
            "acceptance": [
                { "task": "T1", "title": "改", "status": "succeeded", "criteria": 2, "passed": 1,
                  "checks": [{ "criterion": "一", "passed": true }, { "criterion": "二", "passed": false }], "risks": ["没跑全量"] },
                { "task": "T2", "title": "写", "status": "succeeded", "criteria": 1, "passed": 1, "unverified": true,
                  "traceGaps": ["改了 c.rs"], "risks": ["推断"] },
                { "task": "T4", "title": "测", "status": "succeeded", "criteria": 1, "passed": 1, "unverified": true },
            ],
        }),
    );
    let s = log.view().review.unwrap().signals;
    assert!(s[0].source == "lead" && s[0].kind == SignalKind::Decision && s[0].text == "被忽略的文件要带回吗？");
    assert!(
        s.iter().any(|x| x.source == "janus" && x.text == "T3：失败，没有交付结果" && x.task.as_deref() == Some("T3"))
    );
    assert!(
        s.iter().any(|x| x.text == "T1 有 1 条验收标准没通过" && x.task.as_deref() == Some("T1")),
        "a criterion that did not pass"
    );
    assert!(s.iter().any(|x| x.text.contains("T2") && x.detail == "改了 c.rs"), "the trace gap");
    assert!(s.iter().any(|x| x.text.starts_with("T2、T4")), "tasks without an independent verification, in one line");
    assert!(s.iter().any(|x| x.detail == "a.rs、b.rs"), "the lead's own edits");
    assert!(
        !s.iter().any(|x| x.text.contains("没跑全量") || x.text.contains("推断")),
        "the executors' own risks are not here"
    );
    let en = log.view_in(Lang::En, true, &json!({})).review.unwrap().signals;
    assert!(en.iter().any(|x| x.text.starts_with("T2, T4")));
    assert!(en.iter().any(|x| x.detail == "a.rs, b.rs"));
    assert!(en.iter().any(|x| x.text == "T1: 1 criteria not met"));
}
