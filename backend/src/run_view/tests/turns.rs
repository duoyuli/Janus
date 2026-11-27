//! A conversation in turns, as the app's tests had it (`ChatTurnsTests`), and how a client applies
//! the person's clicks to the rows (`shown_rows`).

use super::super::turns::{self, Gap, Options, Row, Stats};
use super::*;
use std::collections::HashMap;

/// A task session's events: `m1` on `T1` unless said otherwise, one second apart.
fn add(log: &mut Log, kind: &str, d: Value) {
    log.add(ev(kind).actor("m1").task("T1").data(d));
}

/// An attempt with Janus's system layer, then one turn: thinking, two commands, an edit, a remark
/// between them, and the reply.
fn turn() -> Log {
    let mut l = Log::default();
    l.add(ev("attempt.started").to("m1").task("T1").data(json!({ "runtime": "Claude Code", "prompt": "做 T1" })));
    l.add(ev("session.prompt").to("m1").task("T1").data(json!({ "channel": "system", "prompt": "协作约定" })));
    l.add(ev("session.prompt").to("m1").task("T1").data(json!({ "channel": "instructions", "prompt": "指令" })));
    add(&mut l, "agent.thought", json!({ "text": "先跑测试" }));
    add(
        &mut l,
        "tool.call",
        json!({ "callId": "c1", "tool": "execute", "input": "pytest -q", "output": "3 passed", "status": "completed" }),
    );
    add(&mut l, "agent.message", json!({ "text": "测试都过了，" }));
    add(&mut l, "agent.message", json!({ "text": "接着改导出。" }));
    add(
        &mut l,
        "tool.call",
        json!({ "callId": "c2", "tool": "edit", "input": "export.py", "status": "completed",
                "diffs": [{ "path": "/w/export.py", "old": "a\nb", "new": "a\nc\nd" }] }),
    );
    add(
        &mut l,
        "tool.call",
        json!({ "callId": "c3", "tool": "execute", "input": "pytest -q", "output": "4 passed", "status": "completed" }),
    );
    add(&mut l, "agent.message", json!({ "text": "导出加好了，" }));
    add(&mut l, "agent.message", json!({ "text": "测试 4 个都过。" }));
    l
}

fn counts(log: &Log) -> impl Fn(i64, usize) -> (usize, usize) + '_ {
    move |seq, i| {
        let e = log.events.iter().find(|e| e.seq == seq).unwrap();
        let x = &e.data.as_ref().unwrap()["diffs"][i];
        crate::diff::line_counts(&text(&x["old"]), &text(&x["new"]))
    }
}

/// The rows of `log` at a level, `busy` when its newest turn runs.
fn rows_of(log: &Log, detail: bool, busy: bool, cwd: &str) -> Vec<Row> {
    let evs: Vec<&Event> = log.events.iter().collect();
    let c = counts(log);
    turns::rows(&evs, &Options { detail, busy, cwd, lang: Lang::Zh, counts: &c })
}

/// What a client shows: the rows with the person's clicks (`open`, by row id) applied.
fn shown_rows(rows: &[Row], open: &HashMap<i64, bool>) -> Vec<Row> {
    let heads: HashMap<i64, Option<bool>> = rows
        .iter()
        .filter_map(|r| match r {
            Row::Head { turn, open: o, id, .. } => Some((*turn, o.map(|o| *open.get(id).unwrap_or(&o)))),
            _ => None,
        })
        .collect();
    let folded = |turn: &i64| heads.get(turn).copied().flatten() == Some(false);
    rows.iter()
        .filter(|r| match r {
            Row::Item { turn: Some(t), fold: true, .. } | Row::Text { turn: t, fold: true, .. } => !folded(t),
            Row::Activity { turn, fold: true, part_of, .. } => {
                !folded(turn) && part_of.is_none_or(|p| !open.contains_key(&p))
            }
            _ => true,
        })
        .cloned()
        .map(|r| match r {
            Row::Head { id, open: Some(o), turn, agent, running, start, end, stats, summary, gap } => Row::Head {
                id,
                open: Some(*open.get(&id).unwrap_or(&o)),
                turn,
                agent,
                running,
                start,
                end,
                stats,
                summary,
                gap,
            },
            Row::Activity { id, events, open: o, summary, turn, fold, whole, sole, part_of, gap } => {
                let (events, o) = match (open.get(&id), &whole) {
                    (Some(&t), Some(all)) => (all.clone(), if sole { None } else { Some(t) }),
                    (Some(&t), None) => (events, o.map(|_| t)),
                    (None, _) => (events, o),
                };
                Row::Activity { id, events, open: o, summary, turn, fold, whole, sole, part_of, gap }
            }
            r => r,
        })
        .collect()
}

fn kinds(log: &Log, rows: &[Row]) -> Vec<String> {
    let kind = |seq: i64| log.events.iter().find(|e| e.seq == seq).unwrap().kind.clone();
    let state = |o: &Option<bool>| match o {
        Some(true) => "open",
        Some(false) => "folded",
        None => "-",
    };
    rows.iter()
        .map(|r| match r {
            Row::Section { seq, injected, .. } => format!("section:{}+{}", kind(*seq), injected.len()),
            Row::Item { seq, .. } => format!("item:{}", kind(*seq)),
            Row::Head { open, .. } => format!("head:{}", state(open)),
            Row::Activity { events, open, .. } => format!("activity:{}:{}", events.len(), state(open)),
            Row::Text { events, reply, .. } => format!("text:{}{}", events.len(), if *reply { ":reply" } else { "" }),
            Row::Edits { files, .. } => format!("edits:{}", files.len()),
        })
        .collect()
}

fn none() -> HashMap<i64, bool> {
    HashMap::new()
}

/// A finished turn folds to the agent's line: the reply and the files it changed stay, the work
/// opens on a click. Janus's injected layer stays in the flow under its attempt.
#[test]
fn a_finished_turn_folds_to_its_reply() {
    let log = turn();
    let rows = rows_of(&log, false, false, "");
    assert_eq!(
        kinds(&log, &shown_rows(&rows, &none())),
        ["section:attempt.started+2", "head:folded", "text:2:reply", "edits:1"]
    );
    let Row::Head { start, end, stats, summary, agent, .. } = &rows[1] else { panic!("no head") };
    assert_eq!(end - start, 7000, "from its first event to its last");
    assert_eq!(stats, "运行 2 条命令 · 改 1 个文件");
    assert_eq!(summary, "工作了 7 秒 · 运行 2 条命令 · 改 1 个文件");
    assert_eq!(agent, "m1");
    let Row::Edits { files, .. } = rows.last().unwrap() else { panic!("no edits") };
    assert_eq!((files[0].added, files[0].removed), (2, 1));
    assert_eq!(files[0].changes[0].seq, 8);
}

/// Opened (by the person, or the detail level), the work shows in order: calls grouped, the
/// remark between them quieter than the reply; the person's own click wins over the level.
#[test]
fn an_open_turn_shows_its_work_in_order() {
    let log = turn();
    let rows = rows_of(&log, true, false, "");
    let open = [
        "section:attempt.started+2",
        "head:open",
        "activity:2:open",
        "text:2",
        "activity:2:open",
        "text:2:reply",
        "edits:1",
    ];
    assert_eq!(kinds(&log, &shown_rows(&rows, &none())), open);
    let head = turns::head_id(log.events[3].seq);
    assert_eq!(
        kinds(&log, &shown_rows(&rows, &HashMap::from([(head, false)]))),
        ["section:attempt.started+2", "head:folded", "text:2:reply", "edits:1"]
    );
    // Opening the folded turn of the summary level shows what the detail level shows, its calls folded.
    let summary = rows_of(&log, false, false, "");
    assert_eq!(
        kinds(&log, &shown_rows(&summary, &HashMap::from([(head, true)]))),
        [
            "section:attempt.started+2",
            "head:open",
            "activity:2:folded",
            "text:2",
            "activity:2:folded",
            "text:2:reply",
            "edits:1"
        ]
    );
    let gaps: Vec<Gap> = rows
        .iter()
        .map(|r| match r {
            Row::Section { gap, .. }
            | Row::Item { gap, .. }
            | Row::Head { gap, .. }
            | Row::Activity { gap, .. }
            | Row::Text { gap, .. }
            | Row::Edits { gap, .. } => *gap,
        })
        .collect();
    assert_eq!(gaps, [Gap::None, Gap::Block, Gap::Line, Gap::Loose, Gap::Line, Gap::Block, Gap::Block]);
}

/// While the agent is at it, the turn stays open with its newest calls, and nothing is called its
/// reply yet.
#[test]
fn a_running_turn_stays_open() {
    let mut log = turn();
    log.events.truncate(log.events.len() - 2);
    let rows = rows_of(&log, false, true, "");
    assert_eq!(
        kinds(&log, &shown_rows(&rows, &none())),
        ["section:attempt.started+2", "head:open", "activity:2:folded", "text:2", "activity:2:open", "edits:1"]
    );
    let Row::Head { running, summary, .. } = &rows[1] else { panic!() };
    assert!(*running && summary.is_empty(), "a running turn's line is its clock and its stats");
}

/// What must not be missed stays when the work folds: a request, a message cut into the turn, a
/// result handed in; a person's message opens the next turn.
#[test]
fn pinned_items_stay_when_the_turn_folds() {
    let mut log = turn();
    log.events.truncate(log.events.len() - 2);
    add(&mut log, "approval.requested", json!({ "key": "k1" }));
    add(
        &mut log,
        "tool.call",
        json!({ "callId": "c4", "tool": "execute", "input": "git push", "status": "completed" }),
    );
    add(&mut log, "agent.message", json!({ "text": "推上去了。" }));
    add(&mut log, "result.submitted", json!({ "summary": "完成" }));
    log.add(ev("user.message").actor("user").to("m1").data(json!({ "text": "再看看", "delivery": "prompt" })));
    add(&mut log, "agent.message", json!({ "text": "好的。" }));
    let rows = rows_of(&log, false, false, "");
    assert_eq!(
        kinds(&log, &shown_rows(&rows, &none())),
        [
            "section:attempt.started+2",
            "head:folded",
            "item:approval.requested",
            "text:1:reply",
            "item:result.submitted",
            "edits:1",
            "item:user.message",
            "head:-",
            "text:1:reply"
        ]
    );
}

/// Calls that are all a turn did need no line of their own under the turn's.
#[test]
fn calls_alone_sit_under_the_turns_line() {
    let mut log = Log::default();
    add(&mut log, "tool.call", json!({ "callId": "c1", "tool": "read", "input": "a.py", "status": "completed" }));
    add(&mut log, "tool.started", json!({ "callId": "c2", "tool": "execute", "input": "pytest", "live": true }));
    assert_eq!(kinds(&log, &shown_rows(&rows_of(&log, false, true, ""), &none())), ["head:open", "activity:2:-"]);
}

/// A file named by its path in the working directory and by its full path is one file.
#[test]
fn a_file_counts_once_whatever_its_path() {
    let mut log = Log::default();
    add(
        &mut log,
        "tool.call",
        json!({ "callId": "c1", "tool": "edit", "status": "completed", "paths": ["pay.js"],
                "diffs": [{ "path": "/w/pay.js", "old": "", "new": "x" }] }),
    );
    add(&mut log, "agent.message", json!({ "text": "改好了" }));
    let evs: Vec<&Event> = log.events.iter().collect();
    assert_eq!(Stats::of(&evs, "/w").edited, 1);
}

/// Janus's prompt that carries the person's message is part of the turn the message opened.
#[test]
fn the_prompt_carrying_a_message_stays_in_its_turn() {
    let mut log = Log::default();
    log.add(ev("user.message").actor("user").to("lead").data(json!({ "text": "接着做", "delivery": "prompt" })));
    log.add(ev("session.prompt").to("lead").data(json!({ "channel": "followup", "prompt": "继续：接着做" })));
    log.add(ev("agent.message").actor("lead").data(json!({ "text": "好。" })));
    let rows = rows_of(&log, false, false, "");
    assert_eq!(
        kinds(&log, &shown_rows(&rows, &none())),
        ["item:user.message", "item:session.prompt", "head:-", "text:1:reply"]
    );
    let Row::Item { gap, .. } = &rows[1] else { panic!() };
    assert_eq!(*gap, Gap::Half, "a prompt right under the message it carries sits with it");
}

/// A turn that only says where the session stands has no work to fold: its status line stays.
#[test]
fn a_status_line_is_not_work() {
    let mut log = Log::default();
    log.add(ev("agent.status").actor("lead").text("会话已打开，等待你的消息").data(json!({ "turn": "idle" })));
    assert_eq!(kinds(&log, &rows_of(&log, false, false, "")), ["item:agent.status"]);
}

/// While a turn runs, the two levels differ at the bottom too: the summary folds the newest calls
/// to one line and shows only the step the agent is on; the detail lists every one. Opened by
/// hand, the summary lists them all like the detail.
#[test]
fn the_levels_differ_where_a_running_turn_is() {
    let mut log = Log::default();
    add(&mut log, "agent.thought", json!({ "text": "先看看" }));
    add(&mut log, "tool.call", json!({ "callId": "c1", "tool": "read", "input": "a.py", "status": "completed" }));
    add(&mut log, "tool.call", json!({ "callId": "c2", "tool": "execute", "input": "pytest", "status": "completed" }));
    add(&mut log, "tool.started", json!({ "callId": "c3", "tool": "execute", "input": "curl", "live": true }));
    let summary = rows_of(&log, false, true, "");
    assert_eq!(kinds(&log, &shown_rows(&summary, &none())), ["head:open", "activity:3:folded", "activity:1:-"]);
    assert_eq!(kinds(&log, &shown_rows(&rows_of(&log, true, true, ""), &none())), ["head:open", "activity:4:-"]);
    let Row::Activity { id, summary: line, .. } = &summary[1] else { panic!() };
    assert_eq!(line, "运行 1 条命令 · 读 1 个文件 · 思考");
    assert_eq!(kinds(&log, &shown_rows(&summary, &HashMap::from([(*id, true)]))), ["head:open", "activity:4:-"]);
}

/// Janus's own board tools show once: an agent's echo of them is hidden, whichever way the agent
/// names them; a running call that came back shows as done; and what sits beside the conversation
/// (plan, controls, commands, usage) is not in it.
#[test]
fn echoes_and_side_facts_are_left_out() {
    let mut log = Log::default();
    let call = |kind: &str, text: &str, tool: &str| {
        ev(kind).actor("lead").text(text).data(json!({ "tool": tool, "input": "{\"timeout_sec\":45}", "callId": "c1" }))
    };
    log.add(call("tool.call", "mcp__janus__wait", "other"));
    log.add(call("tool.started", "mcp.janus.wait", "other"));
    log.add(call("tool.started", "调用 janus.wait，等待返回", "janus.wait"));
    log.add(call("tool.call", "Read calc.py", "read"));
    log.add(ev("agent.plan").actor("lead").data(json!({ "entries": [] })));
    let evs: Vec<&Event> = log.events.iter().collect();
    let shown: Vec<i64> = turns::shown(&evs, &RunState::default(), &[]).iter().map(|e| e.seq).collect();
    assert_eq!(shown, [4], "the board tool's own record came back as done (the read's call id), so it is left out too");
    let mut log = Log::default();
    log.add(call("tool.started", "调用 janus.wait，等待返回", "janus.wait"));
    let evs: Vec<&Event> = log.events.iter().collect();
    assert_eq!(turns::shown(&evs, &RunState::default(), &[]).len(), 1, "Janus's own record stays");
}

/// A steered message is shown where it was inserted: applying it only updates that bubble; a
/// Loop's session opens with its first wake, not with the run.
#[test]
fn a_steered_message_is_shown_once() {
    let mut log = Log::default();
    log.add(ev("run.created").actor("user").data(json!({ "kind": "loop", "cwd": "/tmp/ws" })));
    log.add(ev("loop.woke").data(json!({ "reason": "timer", "n": 3 })));
    log.add(
        ev("user.message")
            .actor("user")
            .to("lead")
            .data(json!({ "id": "m1", "text": "先看 T1", "delivery": "pending" })),
    );
    log.add(ev("message.routed").to("lead").data(json!({ "id": "m1", "delivery": "steer" })));
    log.add(ev("message.delivered").to("lead").data(json!({ "ids": ["m1"], "applied": true })));
    let st = log.state();
    let evs: Vec<&Event> = log.events.iter().collect();
    let shown: Vec<i64> = turns::shown(&evs, &st, &[]).iter().map(|e| e.seq).collect();
    assert_eq!(shown, [2, 4], "the wake, and the message where it was inserted");
}
