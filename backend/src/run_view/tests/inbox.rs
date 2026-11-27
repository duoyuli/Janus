//! The inbox: what each run waits on the person for, and what the person did with its entries.

use super::review::{ask, delivered};
use super::*;
use crate::db::{InboxMark, RunRow};
use crate::run_view::inbox::{Candidate, inbox_entries};
use crate::status::RunOrigin;

fn row(id: &str, kind: RunKind, status: RunStatus) -> RunRow {
    RunRow {
        id: id.into(),
        goal: "上线支付页".into(),
        cwd: "/tmp/x".into(),
        config: team(),
        status,
        created_at: 1_000_000,
        kind,
        origin: RunOrigin::User,
        loop_id: None,
        dispatch_key: None,
        source_url: None,
        queue_position: None,
    }
}

fn team() -> Value {
    json!({ "team": [{ "id": "m1", "name": "实现者" }], "workspace": { "id": "w1", "name": "商城" } })
}

/// A run's entries, its events read as a live run's.
fn entries(log: &Log, status: RunStatus) -> Vec<InboxEntry> {
    entries_in(log, status, Lang::Zh)
}

fn entries_in(log: &Log, status: RunStatus, lang: Lang) -> Vec<InboxEntry> {
    let r = row("r1", RunKind::Goal, status);
    let c = Candidate { row: &r, workspace_name: "商城".into() };
    // What the service reads for the inbox: the person's items only, not the whole view.
    let words = Words::of(lang, &team());
    let asks = fold(&log.events).asks(true, &words);
    // The run page's 「需要你处理」 says the same.
    let view = log.view_in(lang, true, &team());
    let page: Vec<_> = view.board.open.iter().filter(|o| o.you).map(|o| serde_json::to_value(o).unwrap()).collect();
    let mut read: Vec<_> = asks.open.iter().map(|o| serde_json::to_value(o).unwrap()).collect();
    read.sort_by_key(|o| std::cmp::Reverse(o["seq"].as_i64()));
    assert_eq!(read, page, "the inbox reads what the run page shows");
    assert_eq!(serde_json::to_value(&asks.review).unwrap(), serde_json::to_value(&view.review).unwrap());
    inbox_entries(&c, Some(&asks), &words)
}

/// What a run's record alone asks.
fn row_entries(id: &str, kind: RunKind, status: RunStatus) -> Vec<InboxEntry> {
    let r = row(id, kind, status);
    let c = Candidate { row: &r, workspace_name: "W".into() };
    inbox_entries(&c, None, &Words::of(Lang::Zh, &team()))
}

fn kinds(e: &[InboxEntry]) -> Vec<&str> {
    e.iter().map(|x| x.kind.as_str()).collect()
}

fn start() -> Log {
    let mut log = Log::default();
    log.add(ev("run.created").actor("user").data(json!({ "mode": "goal", "goal": "上线支付页" })));
    log
}

/// The inbox shows exactly the run page's 「需要你处理」, and only what nothing waits on can be put off.
#[test]
fn the_inbox_takes_the_persons_items_from_a_live_run() {
    let mut log = start();
    log.add(ev("orchestration.accepted").actor("lead").data(json!({ "tasks": [
        { "id": "T1", "title": "实现", "agent": "m1", "deps": [], "accept": ["能付款"] },
    ]})));
    log.add(ev("attempt.started").task("T1"));
    log.add(ev("approval.requested").actor("m1").task("T1").data(json!({ "key": "a-1", "command": "npm publish" })));
    log.add(
        ev("question.escalated")
            .actor("lead")
            .data(json!({ "key": "q-lead-1", "question": "先上哪个地区？", "options": ["国内", "海外"] })),
    );
    // The lead's own blocker: the lead settles it, it is not the person's.
    log.add(ev("board.posted").actor("m1").task("T1").data(json!({ "kind": "blocker", "text": "缺少测试账号" })));
    let e = entries(&log, RunStatus::Running);
    assert_eq!(e.len(), 2);
    let approval = e.iter().find(|x| x.kind == "approval").unwrap();
    let decision = e.iter().find(|x| x.kind == "question").unwrap();
    assert!(!approval.can_wait && approval.text == "npm publish" && approval.workspace_name == "商城");
    assert!(decision.can_wait && decision.ts == 1_004_000);
    assert!(e.iter().all(|x| x.key.starts_with("r1/") && x.goal == "上线支付页" && x.workspace == "w1"));
}

#[test]
fn a_finished_run_asks_for_a_final_word() {
    // A delivered run is read like a live one: its delivery speaks for it.
    assert!(row_entries("r2", RunKind::Goal, RunStatus::Delivered).is_empty());
    let stopped = row_entries("r3", RunKind::Goal, RunStatus::Interrupted);
    assert!(stopped.len() == 1 && stopped[0].can_wait && stopped[0].key == "r3/interrupted");
    assert_eq!(stopped[0].title, "已中断，可以从中断处继续");
    assert_eq!(row_entries("r3", RunKind::Goal, RunStatus::Failed)[0].kind, "failed");
    assert!(row_entries("r4", RunKind::Goal, RunStatus::Accepted).is_empty());
    assert!(row_entries("r5", RunKind::Goal, RunStatus::Cancelled).is_empty());
}

/// A Loop's session closes between wakes, and a chat's between messages: that is how they rest.
#[test]
fn resting_loops_and_chats_ask_nothing() {
    assert!(row_entries("l1", RunKind::Loop, RunStatus::Interrupted).is_empty());
    assert!(row_entries("c1", RunKind::Chat, RunStatus::Interrupted).is_empty());
    assert_eq!(row_entries("r6", RunKind::Goal, RunStatus::Interrupted).len(), 1);
}

/// A delivery is the lead's turn ending: the run asks for acceptance once, not also for a reply,
/// whichever of the two events comes first. Before the delivery, a turn that ends still asks.
#[test]
fn a_delivered_run_asks_once_not_also_for_a_reply() {
    let idle = json!({ "phase": "waiting_for_input", "turn": "idle" });
    let mut asking = start();
    asking.add(ev("agent.status").actor("lead").data(idle.clone()));
    assert_eq!(kinds(&entries(&asking, RunStatus::Running)), ["session"]);
    for idle_first in [true, false] {
        let mut log = start();
        if idle_first {
            log.add(ev("agent.status").actor("lead").data(idle.clone()));
        }
        log.add(ev("run.delivered").actor("lead").data(json!({ "summary": "好了" })));
        if !idle_first {
            log.add(ev("agent.status").actor("lead").data(idle.clone()));
        }
        assert_eq!(kinds(&entries(&log, RunStatus::Delivered)), ["delivered"]);
    }
}

/// The inbox asks for a delivery only where the person has to act, one entry per delivery.
#[test]
fn the_inbox_asks_for_a_delivery_by_its_stage() {
    let done = json!({ "headline": "做完了", "summary": "改了 a.rs" });
    let mut first = start();
    delivered(&mut first, done.clone());
    let e = entries(&first, RunStatus::Delivered);
    assert_eq!(e.iter().map(|x| x.key.as_str()).collect::<Vec<_>>(), ["r1/delivered-1"]);
    assert!(e[0].text == "做完了" && e[0].title == "已交付，等你验收" && e[0].sub == "没有需要你看的，核对后验收");
    let mut working = first.clone();
    ask(&mut working, "再改");
    assert!(entries(&working, RunStatus::Delivered).is_empty(), "the lead works on it");
    delivered(&mut working, done.clone());
    let second = entries(&working, RunStatus::Delivered);
    assert_eq!(
        second.iter().map(|x| x.key.as_str()).collect::<Vec<_>>(),
        ["r1/delivered-2"],
        "a new delivery is a new entry"
    );
    assert_eq!(second[0].title, "第 2 次交付，等你验收");
    assert_eq!(
        entries_in(&working, RunStatus::Delivered, Lang::En)[0].title,
        "Delivery 2, waiting for your acceptance"
    );
    let mut held = first.clone();
    ask(&mut held, "再查");
    held.add(ev("session.interrupted").actor("user").to("lead"));
    held.add(ev("agent.status").actor("lead").data(json!({ "phase": "waiting_for_input", "turn": "idle" })));
    assert_eq!(kinds(&entries(&held, RunStatus::Delivered)), ["held"]);
    let mut conflict = start();
    delivered(&mut conflict, json!({ "merge": { "state": "conflict", "note": "仍然冲突" } }));
    let e = entries(&conflict, RunStatus::Delivered);
    assert!(kinds(&e) == ["merge_blocked"] && e[0].text == "仍然冲突");
    let mut accepted = first.clone();
    accepted.add(ev("risk.accepted").actor("user"));
    assert!(entries(&accepted, RunStatus::Accepted).is_empty());
}

/// A plain choice is answered in the inbox: an approval's options in the app's words, and a
/// decision's options with the lead's pick standing out; each click is the run page's own action.
#[test]
fn plain_choices_can_be_answered_from_the_inbox() {
    let mut log = start();
    log.add(ev("orchestration.accepted").actor("lead").data(json!({ "tasks": [
        { "id": "T1", "title": "清理旧构建", "agent": "m1", "deps": [], "accept": ["干净"] },
    ]})));
    log.add(ev("attempt.started").task("T1"));
    log.add(ev("approval.requested").actor("m1").task("T1").data(
        json!({ "key": "a-1", "command": "rm -rf build", "options": [
            { "optionId": "o1", "name": "Allow once", "kind": "allow_once" },
            { "optionId": "o2", "name": "Always allow", "kind": "allow_always" },
            { "optionId": "o3", "name": "Reject", "kind": "reject_once" },
        ]}),
    ));
    log.add(ev("question.escalated").actor("lead").data(json!({
        "key": "q-lead-1", "question": "先上哪个地区？", "options": ["国内", "海外"], "recommend": "海外",
    })));
    log.add(
        ev("input.requested")
            .actor("m1")
            .task("T1")
            .data(json!({ "key": "i-1", "message": "账号", "schema": { "type": "object" } })),
    );
    let e = entries(&log, RunStatus::Running);
    let approval = e.iter().find(|x| x.kind == "approval").unwrap();
    assert_eq!(approval.title, "「实现者」等你拍板", "named like the request card");
    assert_eq!(approval.sub, "T1 · 清理旧构建");
    let labels: Vec<&str> = approval.actions.iter().map(|a| a.label.as_str()).collect();
    assert_eq!(labels, ["允许一次", "始终允许", "拒绝"]);
    let a = &approval.actions;
    assert!(a[0].prominent && a[0].body["option"] == "o1" && a[0].body["allow"] == true);
    assert!(
        a[0].body["type"] == "approve"
            && a[0].body["key"] == "a-1"
            && a[0].body["task"] == "T1"
            && a[0].body["agent"] == "m1"
    );
    assert!(a[2].body["allow"] == false && a[2].body["label"] == "Reject", "the agent's own words go back");
    assert!(a[1].help.contains("不再询问") && a[1].help.contains("Always allow"), "{}", a[1].help);
    let decision = e.iter().find(|x| x.kind == "question").unwrap();
    assert_eq!(decision.title, "负责人等你拍板：先上哪个地区？");
    assert_eq!(decision.actions.iter().map(|a| a.label.as_str()).collect::<Vec<_>>(), ["国内", "海外"]);
    assert!(
        decision.actions[1].prominent
            && decision.actions[1].help == "负责人建议"
            && decision.actions[0].help.is_empty()
    );
    assert!(
        decision.actions[0].body["type"] == "answer"
            && decision.actions[0].body["key"] == "q-lead-1"
            && decision.actions[0].body["task"] == ""
    );
    let form = e.iter().find(|x| x.key.ends_with("input")).unwrap();
    assert!(form.actions.is_empty(), "a form opens the run");
    assert_eq!(form.title, "「实现者」需要你填一张表");
    let en = entries_in(&log, RunStatus::Running, Lang::En);
    let approval = en.iter().find(|x| x.kind == "approval").unwrap();
    assert_eq!(approval.title, "实现者 needs your call");
    assert_eq!(
        approval.actions.iter().map(|a| a.label.as_str()).collect::<Vec<_>>(),
        ["Allow Once", "Always Allow", "Deny"]
    );
    assert_eq!(en.iter().find(|x| x.kind == "question").unwrap().actions[1].help, "Lead’s recommendation");
}

#[test]
fn permission_options_without_options_allow_or_reject_once() {
    let w = Words::of(Lang::Zh, &json!({}));
    let opts = w.permission_options(&Value::Null);
    assert_eq!(opts.iter().map(|o| o.label.as_str()).collect::<Vec<_>>(), ["允许一次", "拒绝"]);
    assert!(opts.iter().all(|o| o.id.is_empty()));
    let other = w.permission_options(&json!([{ "optionId": "x", "name": "Proceed", "kind": "other" }]));
    assert_eq!(other[0].label, "Proceed", "an unknown kind keeps the agent's words");
}

/// A consolidation of the team memory asks only to be looked at; while it works its row asks
/// nothing.
#[test]
fn a_consolidation_asks_to_be_looked_at() {
    let done = row_entries("d1", RunKind::Dream, RunStatus::Delivered);
    assert!(done.len() == 1 && done[0].kind == "dream" && done[0].can_wait);
    assert_eq!(done[0].title, "团队记忆整理好了");
    let broke = row_entries("d2", RunKind::Dream, RunStatus::Failed);
    assert!(broke.len() == 1 && broke[0].kind == "dream_failed" && broke[0].can_wait);
    assert!(row_entries("d3", RunKind::Dream, RunStatus::Running).is_empty());
}

/// The service keeps what the person did with each entry: a snooze moves an entry that can wait to
/// `later` until its time, one that holds an agent up stays open whatever is asked; an ignored one
/// is gone; the marks of entries that are no longer there are forgotten.
#[test]
fn the_service_keeps_what_the_person_did_with_each_entry() {
    let dir = std::env::temp_dir().join(format!("janus-inbox-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let app = crate::orchestrator::test_app(&dir);
    let db = &app.db;
    db.insert_run("r1", RunKind::Goal, "上线支付页", "/tmp/x", &team(), 1_000_000).unwrap();
    db.set_run_status("r1", RunStatus::Running).unwrap();
    db.insert_run("r2", RunKind::Goal, "修复", "/tmp/x", &team(), 2_000_000).unwrap();
    db.set_run_status("r2", RunStatus::Interrupted).unwrap();
    let mut log = start();
    log.add(ev("orchestration.accepted").actor("lead").data(json!({ "tasks": [
        { "id": "T1", "title": "实现", "agent": "m1", "deps": [], "accept": ["能付款"] },
    ]})));
    log.add(ev("attempt.started").task("T1"));
    log.add(ev("approval.requested").actor("m1").task("T1").data(json!({ "key": "a-1", "command": "npm publish" })));
    log.add(
        ev("question.escalated")
            .actor("lead")
            .data(json!({ "key": "q-1", "question": "先上哪个？", "options": ["国内"] })),
    );
    for e in &log.events {
        db.insert_event(e).unwrap();
    }
    let now = 5_000_000;
    let inbox = app.inbox(Lang::Zh, now).unwrap();
    let keys = |v: &[InboxEntry]| v.iter().map(|e| e.key.clone()).collect::<Vec<_>>();
    assert_eq!(inbox.open.len(), 3, "{:?}", keys(&inbox.open));
    let question = inbox.open.iter().find(|e| e.kind == "question").unwrap().key.clone();
    let approval = inbox.open.iter().find(|e| e.kind == "approval").unwrap().key.clone();
    assert_eq!(inbox.open[0].key, "r2/interrupted", "newest first");
    // The workspace's page folds them into one row per goal, in the same order.
    let rows = &inbox.by_workspace["w1"];
    assert_eq!(rows.iter().map(|w| (w.run.as_str(), w.entries.len())).collect::<Vec<_>>(), [("r2", 1), ("r1", 2)]);
    assert_eq!((rows[0].title.as_str(), rows[0].label.as_str()), ("修复", "已中断"));
    assert_eq!(rows[1].label, "2 件事");

    app.mark_inbox(&question, InboxMark { snoozed_until: Some(now + 60_000), ignored: false }).unwrap();
    // An approval holds an agent up: putting it off does not move it.
    app.mark_inbox(&approval, InboxMark { snoozed_until: Some(now + 60_000), ignored: false }).unwrap();
    app.mark_inbox("r2/interrupted", InboxMark { snoozed_until: None, ignored: true }).unwrap();
    let inbox = app.inbox(Lang::Zh, now).unwrap();
    assert_eq!(keys(&inbox.open), std::slice::from_ref(&approval));
    assert_eq!(keys(&inbox.later), std::slice::from_ref(&question));
    assert_eq!(inbox.next_wake, Some(now + 60_000));
    assert_eq!(inbox.later[0].snoozed_until, Some(now + 60_000));
    // Its time comes: it is back.
    let back = app.inbox(Lang::Zh, now + 61_000).unwrap();
    assert!(back.later.is_empty() && back.open.iter().any(|e| e.key == question));

    // Answered elsewhere: the entry and its mark are gone.
    db.insert_event(
        &ev("question.answered").actor("user").to("lead").data(json!({ "key": "q-1", "answer": "国内" })).0,
    )
    .unwrap();
    let inbox = app.inbox(Lang::Zh, now).unwrap();
    assert!(!inbox.open.iter().chain(&inbox.later).any(|e| e.key == question));
    let marks = db.inbox_marks().unwrap();
    assert!(!marks.contains_key(&question), "the mark of a gone entry is forgotten");
    assert!(marks["r2/interrupted"].ignored, "an ignored entry that is still there keeps its mark");
    let _ = std::fs::remove_dir_all(&dir);
}
