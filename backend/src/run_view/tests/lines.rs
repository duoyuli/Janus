//! Each event of a conversation in one line, each request's card and each composer, as the app's
//! `ChatItem`, `ToolRow`, `RequestCard` and `ChatCard.mode` wrote them.

use super::super::cards::{CardKind, FieldKind};
use super::super::lines::{self, Line};
use super::super::sessions::Act;
use super::*;

fn note_of(v: &RunView, seq: i64) -> String {
    match &v.lines[&seq] {
        Line::Note { text, .. } => text.clone(),
        other => panic!("seq {seq} is no note: {other:?}"),
    }
}

/// An attempt's header names its task, runtime and model; a goal's divider its title; a chat's
/// where it works.
#[test]
fn sections_say_what_they_open() {
    let mut log = goal_run();
    log.events[3].data = Some(json!({ "session": "s1", "runtime": "codex", "config": { "model": "gpt-5" } }));
    let v = log.view();
    match &v.lines[&4] {
        Line::Attempt { title, .. } => assert_eq!(title, "T1 · 尝试 1 · codex · gpt-5"),
        other => panic!("{other:?}"),
    }
    match &v.lines[&1] {
        Line::Divider { text, .. } => assert_eq!(text, "目标 · 给 divide 增加除零保护"),
        other => panic!("{other:?}"),
    }
    let chat = chat_opened().view();
    match &chat.lines[&1] {
        Line::Divider { text, sub, .. } => {
            assert!(text.starts_with("对话 · 研究员 · "), "{text}");
            assert_eq!(sub.as_deref(), Some("工作目录 ~/.janus/chats/r1 · 对话专用，删对话时一起删"));
        }
        other => panic!("{other:?}"),
    }
}

/// A tool call reads as a verb and what it worked on: a command as itself, a file by its name
/// relative to the workspace; Janus's own tools as what they did on the board.
#[test]
fn tool_calls_read_as_a_verb_and_their_object() {
    assert_eq!(
        lines::label("execute", "cargo test\n--all", "", false, "/w", Lang::Zh),
        ("运行".into(), "cargo test".into(), true)
    );
    assert_eq!(
        lines::label("read", "/w/src/lib.rs", "", true, "/w", Lang::En),
        ("Reading".into(), "lib.rs".into(), true)
    );
    assert_eq!(
        lines::label("edit", "src/a.rs 10-20", "改 a.rs", false, "/w", Lang::Zh),
        ("改".into(), "改 a.rs".into(), false)
    );
    assert_eq!(lines::label("other", "x", "自定义", false, "/w", Lang::Zh), (String::new(), "自定义".into(), false));
    assert_eq!(lines::in_workspace("/w/a/b.rs and /w/c", "/w/"), "a/b.rs and c");

    let mut log = goal_run();
    log.add(ev("tool.call").actor("researcher").task("T1").text("读 lib.rs").data(json!({
        "tool": "read", "input": "/tmp/x/src/lib.rs", "output": "```\nfn main() {}\n```", "callId": "c1",
    })));
    log.add(ev("tool.call").actor("lead").data(json!({
        "tool": "janus.orchestrate", "args": { "tasks": [{ "id": "T3", "title": "写测试", "assignee": "tester" }] }, "output": "已派出",
    })));
    let v = log.view();
    match &v.lines[&5] {
        Line::Tool(t) => {
            assert_eq!((t.verb.as_str(), t.what.as_str(), t.mono, t.board), ("读", "lib.rs", true, false));
            assert_eq!(t.summary, "→ fn main() {}");
        }
        other => panic!("{other:?}"),
    }
    match &v.lines[&6] {
        Line::Tool(t) => {
            assert!(t.board && t.tag == "看板");
            assert_eq!(t.what, "派出子任务：T3 写测试 → tester");
            assert_eq!(t.summary, "→ 已派出");
        }
        other => panic!("{other:?}"),
    }
}

/// A message shows in its bubble with how it went in; one cut into the turn says whether the
/// agent read it yet.
#[test]
fn messages_say_how_they_went_in() {
    let mut log = goal_run();
    log.add(ev("user.message").actor("user").to("researcher").task("T1").text("先别改").data(json!({
        "id": "m1", "text": "先别改", "delivery": "pending",
    })));
    log.add(ev("message.routed").to("researcher").task("T1").data(json!({ "id": "m1", "delivery": "steer" })));
    let v = log.view();
    match &v.lines[&6] {
        Line::Bubbles { items } => {
            assert_eq!(items[0].text, "先别改");
            assert!(items[0].insert);
            assert_eq!(items[0].via, "插话 · 插话 · 已插进这一轮，做完手头这步就会读到");
        }
        other => panic!("{other:?}"),
    }
}

/// Notes from Janus carry their mark and colour; a lead's edit after delegating is a warning.
#[test]
fn notes_carry_their_mark() {
    let mut log = goal_run();
    log.add(ev("lead.edited").actor("lead").data(json!({ "paths": ["a.rs", "b.rs"] })));
    log.add(ev("task.succeeded").task("T1").actor("researcher").text("完成"));
    let v = log.view_in(Lang::En, true, &json!({ "team": [{ "id": "researcher", "name": "Ada" }] }));
    assert_eq!(
        note_of(&v, 5),
        "⚠ Edited files after handing out subtasks: a.rs, b.rs · listed on their own in the delivery"
    );
}

/// A waiting approval is a card with the agent's own options; once answered it is one line.
#[test]
fn a_waiting_request_has_its_card() {
    let mut log = goal_run();
    log.add(ev("approval.requested").actor("researcher").task("T1").text("写文件").data(json!({
        "key": "a-1", "title": "写 a.rs", "command": "touch /tmp/x/a.rs", "policy": "改工作区外的文件",
        "options": [{ "optionId": "y", "name": "允许", "kind": "allow_once" }, { "optionId": "n", "name": "拒绝", "kind": "reject_once" }],
    })));
    let v = log.view();
    match &v.lines[&5] {
        Line::Request { key, open, docked, .. } => {
            assert_eq!(key, "a-1");
            assert!(open);
            assert!(docked.ends_with("· 等你处理，在输入框上方"), "{docked}");
        }
        other => panic!("{other:?}"),
    }
    let c = v.cards.iter().find(|c| c.key == "a-1").expect("its card");
    assert_eq!(c.kind, CardKind::Approval);
    assert_eq!(c.title, "「researcher」等你拍板：写 a.rs");
    assert_eq!(c.command, "$ touch a.rs");
    assert_eq!(c.facts[1], ("原因".to_string(), "改工作区外的文件".to_string()));
    assert_eq!(c.actions.len(), 2);
}

/// A form's schema becomes its fields: choices with their labels, the person's own answer beside
/// them, and which ones are required.
#[test]
fn a_form_is_read_into_fields() {
    let mut log = goal_run();
    log.add(ev("input.requested").actor("researcher").task("T1").data(json!({
        "key": "f-1", "message": "选一个环境",
        "schema": {
            "required": ["env"],
            "properties": {
                "env": { "type": "string", "title": "环境", "oneOf": [
                    { "const": "test", "title": "测试 — 先试试" }, { "const": "prod", "title": "生产" },
                ] },
                "env_custom": { "type": "string", "description": "别的环境" },
                "count": { "type": "integer" },
            },
        },
    })));
    let v = log.view();
    let c = v.cards.iter().find(|c| c.key == "f-1").expect("its card");
    assert_eq!(c.kind, CardKind::Form);
    assert_eq!(c.fields.iter().map(|f| f.key.as_str()).collect::<Vec<_>>(), ["count", "env"]);
    let env = &c.fields[1];
    assert!(env.required && env.kind == FieldKind::Choice);
    assert_eq!((env.choices[0].label.as_str(), env.choices[0].desc.as_str()), ("测试", "先试试"));
    assert_eq!(env.custom.as_ref().map(|c| c.placeholder.as_str()), Some("别的环境"));
    assert_eq!(c.fields[0].kind, FieldKind::Number);
}

/// The composer of a chat waiting for the next message sends plainly; of a task at work cuts in.
#[test]
fn composers_offer_what_the_session_takes() {
    let chat = chat_opened().view();
    let m = &chat.sessions["agent:lead"].mode;
    assert_eq!(m.state, "等你回话");
    assert_eq!(m.buttons.iter().map(|b| b.act).collect::<Vec<_>>(), [Act::Message]);
    assert!(m.enabled && m.keys);

    let task = goal_run().view();
    let m = &task.sessions["task:T1"].mode;
    assert_eq!(m.state, "托管中 · Agent 自主推进");
    assert_eq!(m.buttons.iter().map(|b| b.act).collect::<Vec<_>>(), [Act::Interrupt, Act::Takeover, Act::Message]);
    assert!(m.buttons[0].danger && m.buttons[2].primary);
}

/// A Loop's session marks each wake: which one, when and why; then when it ended, how long it took
/// and when the next one is.
#[test]
fn a_loops_session_marks_when_each_wake_starts_and_ends() {
    let mut log = Log::default();
    log.add(ev("run.created").actor("user").data(json!({ "kind": "loop", "cwd": "/tmp/ws", "git": false })));
    log.add(ev("loop.woke").data(json!({ "reason": "timer", "n": 3 })));
    log.add(ev("agent.message").actor("lead").data(json!({ "text": "看过了" })));
    log.add(ev("loop.slept").data(json!({ "until": 9_000_000, "reason": "self", "summary": "看过了" })));
    log.add(ev("loop.woke").data(json!({ "reason": "manual", "n": 4 })));
    log.add(ev("loop.slept").data(json!({ "until": null, "reason": null, "summary": "" })));
    let v = log.view();
    match &v.lines[&2] {
        Line::Divider { text, .. } => {
            assert!(text.starts_with("第 3 次开工 · ") && text.ends_with(" · 按时间表"), "{text}")
        }
        other => panic!("{other:?}"),
    }
    let slept = note_of(&v, 4);
    assert!(slept.contains("收工（2 秒）· 下次开工：") && slept.ends_with("（它约的）"), "{slept}");
    // No time ahead: it waits to be called.
    assert!(note_of(&v, 6).ends_with("收工（1 秒）· 下次等你叫它"), "{}", note_of(&v, 6));
}
