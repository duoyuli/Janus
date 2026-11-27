//! What sits beside a conversation (plan, controls, commands, usage, Git facts, messages on their
//! way), what its composer suggests, the live previews, and the run's documents, as the app's
//! tests had them (`SessionViewTests`, `ChatTurnsTests`, `MarkdownDocTests`, `ChatTests`).

use super::super::lines::Line;

use super::super::aside::{self, compact};
use super::super::docs::{DocsFold, Moved, Source, paths_in};
use super::super::preview::Previews;
use super::super::suggest::{self, Asking};
use super::*;
use std::path::Path;

fn task_events(log: &Log) -> Vec<&Event> {
    log.events.iter().collect()
}

fn add(log: &mut Log, kind: &str, d: Value) {
    log.add(ev(kind).actor("m1").task("T1").data(d));
}

/// The plan, controls, commands and usage beside a conversation come from its latest events.
#[test]
fn session_state_comes_from_the_latest_events() {
    let mut log = Log::default();
    add(
        &mut log,
        "session.controls",
        json!({ "model": { "id": "model", "current": "a", "options": [{ "value": "a", "name": "A" }, { "value": "b", "name": "B" }] },
                "controls": [
                    { "id": "mode", "name": "Mode", "category": "mode", "type": "select", "via": "mode", "current": "ask",
                      "options": [{ "value": "ask" }, { "value": "full" }] },
                    { "id": "solo", "type": "select", "current": "x", "options": [{ "value": "x" }] }] }),
    );
    add(
        &mut log,
        "agent.plan",
        json!({ "entries": [{ "content": "读", "status": "completed" }, { "content": "改", "status": "in_progress" }] }),
    );
    add(&mut log, "session.control", json!({ "id": "mode", "value": "full", "via": "mode" }));
    add(&mut log, "session.control", json!({ "id": "model", "failed": true }));
    add(&mut log, "session.commands", json!({ "commands": [{ "name": "compact", "description": "压缩" }] }));
    add(&mut log, "session.usage", json!({ "used": 50000, "size": 200000 }));
    add(&mut log, "session.usage", json!({ "totalTokens": 1500 }));
    let evs = task_events(&log);
    assert_eq!(aside::plan(&evs).iter().map(|s| s.status.as_str()).collect::<Vec<_>>(), ["completed", "in_progress"]);
    let c = aside::controls(&evs, Lang::Zh);
    assert_eq!(c.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["model", "mode"], "one choice is not offered");
    assert!(c[0].current == "a" && c[1].current == "full" && c[1].name == "模式" && c[1].via == "mode");
    assert_eq!(c[0].label, "模型：A");
    assert_eq!(aside::controls(&evs, Lang::En)[1].label, "Mode: full");
    assert_eq!(aside::commands(&evs)[0].name, "compact");
    let u = aside::usage(&evs, Lang::Zh).unwrap();
    assert_eq!((u.percent, u.tokens), (Some(25), Some(1500)));
    assert_eq!(u.labels, "本会话 上下文 25% · 上一轮 1,500 tokens");
    assert_eq!(u.meter, "25%");
    assert_eq!(
        u.help,
        "本会话 上下文 25% · 上一轮 1,500 tokens · 上下文已用 50K · 共 200K\n本会话的数字不计入任务累计用量"
    );
    assert_eq!(aside::usage(&evs, Lang::En).unwrap().labels, "Session context: 25% · Last turn: 1,500 tokens");
    assert!(aside::usage(&[], Lang::Zh).is_none());
}

/// The session's own numbers: an agent that reports 0 before it read anything shows nothing yet,
/// and one with only a last turn shows its tokens.
#[test]
fn session_usage_is_labelled_apart_from_the_run_total() {
    let mut log = Log::default();
    add(&mut log, "session.usage", json!({ "used": 0, "size": 200000 }));
    add(&mut log, "session.usage", json!({ "used": 21000, "size": 200000 }));
    let c = aside::usage(&task_events(&log), Lang::Zh).unwrap();
    assert_eq!((c.tokens, c.percent), (None, Some(10)));
    assert_eq!(c.labels, "本会话 上下文 10%");
    let mut log = Log::default();
    add(&mut log, "session.usage", json!({ "totalTokens": 700 }));
    let t = aside::usage(&task_events(&log), Lang::Zh).unwrap();
    assert_eq!((t.labels.as_str(), t.meter.as_str()), ("上一轮 700 tokens", "700 tokens"));
    assert_eq!(
        (compact(15_400), compact(21_400_000), compact(6_450_000_000), compact(999)),
        ("15.4K".into(), "21.4M".into(), "6.45B".into(), "999".into())
    );
}

/// An attempt's header says which session it is in and why it runs again, and where it works.
#[test]
fn an_attempt_says_how_it_started() {
    let mut log = Log::default();
    log.add(ev("attempt.started").to("m1").task("T1").data(json!({
        "continuation": { "loaded": true, "from": "T1" }, "reopen": { "reason": "lead.retry", "from": "V1" },
        "git": { "repo": "/src/app", "branch": "main", "head": "0123456789abcdef" } })));
    log.add(
        ev("attempt.started").to("m1").task("T2").data(json!({ "continuation": { "fallback": "context_over_75pct" } })),
    );
    log.add(
        ev("run.created")
            .actor("user")
            .data(json!({ "worktree": { "repo": "/src/app", "branch": "" }, "baseRef": "" })),
    );
    let n = aside::section_note(&log.events[0], Lang::Zh).unwrap();
    assert_eq!(n.note, " · 接上原会话 · 按 V1 的意见返工");
    assert_eq!(n.git.as_deref(), Some("Git 仓库 /src/app · 分支 main · 提交 01234567"));
    assert_eq!(
        aside::section_note(&log.events[1], Lang::En).unwrap().note,
        " · New session (previous one was over 75% of its context)"
    );
    assert_eq!(
        aside::section_note(&log.events[2], Lang::Zh).unwrap().git.as_deref(),
        Some("Git 仓库 /src/app · detached HEAD")
    );
    assert_eq!(
        aside::task_git(&task_events(&log), "T1", Lang::En).as_deref(),
        Some("Git repository /src/app · branch main · commit 01234567")
    );
    assert!(aside::task_git(&task_events(&log), "T2", Lang::Zh).is_none(), "outside a worktree");
    let note = |d: Value, task: &str| {
        let mut log = Log::default();
        log.add(ev("attempt.started").task(task).data(d));
        aside::section_note(&log.events[0], Lang::Zh).unwrap().note
    };
    assert_eq!(note(json!({ "session": "s1" }), "T1"), "");
    assert_eq!(note(json!({ "resumed": true }), "T1"), " · 接上了原会话");
    assert_eq!(note(json!({ "continuation": { "from": "T1", "loaded": true } }), "T2"), " · 接上 T1 的会话");
    assert_eq!(
        note(
            json!({ "continuation": { "from": "T4", "loaded": false, "fallback": "no_session" }, "reopen": { "reason": "lead.retry" } }),
            "T4"
        ),
        " · 新开会话（这个子任务之前没有会话）"
    );
    assert_eq!(
        note(
            json!({ "continuation": { "loaded": false, "fallback": "context_over_75pct" }, "reopen": { "reason": "target.reworked", "target": "T1" } }),
            "V1"
        ),
        " · 新开会话（原会话上下文超过 75%） · T1 返工后复审"
    );
}

/// The conversation's view carries all of it, the messages still on their way and the requests
/// waiting on the person.
#[test]
fn a_conversation_carries_what_sits_beside_it() {
    let mut log = goal_run();
    log.add(ev("approval.requested").actor("researcher").task("T1").data(json!({ "key": "a-1", "reason": "写文件" })));
    log.add(
        ev("user.message")
            .actor("user")
            .to("researcher")
            .task("T1")
            .text("先别改")
            .data(json!({ "id": "m1", "text": "先别改", "delivery": "pending" })),
    );
    log.add(ev("message.routed").to("researcher").task("T1").data(json!({ "id": "m1", "delivery": "queued" })));
    let v = log.view();
    let s = &v.sessions["task:T1"];
    assert_eq!(s.requests, ["a-1"]);
    assert_eq!(
        s.pending.iter().map(|p| (p.text.as_str(), p.label.as_str())).collect::<Vec<_>>(),
        [("先别改", "排队 · 这一轮结束后发送")]
    );
    assert!(!s.rows.is_empty() && s.notes.contains_key(&4), "the attempt's section has its note");
    assert!(matches!(v.lines.get(&4), Some(Line::Attempt { .. })), "and its header");
}

/// Typing `/` at the start asks for a command, `@` as the last word for a file; picking one
/// completes that word.
#[test]
fn composer_suggestions_complete_the_word_being_typed() {
    assert_eq!(suggest::asking("/re"), Some(Asking::Command("re".into())));
    assert_eq!(suggest::asking("/review now"), None);
    assert_eq!(suggest::asking("看看 @src/ma"), Some(Asking::File("src/ma".into())));
    assert_eq!(suggest::asking("看看 src"), None);
    assert_eq!(suggest::complete("/re", &Asking::Command("review".into())), "/review ");
    assert_eq!(suggest::complete("看看 @src/ma", &Asking::File("src/main.rs".into())), "看看 @src/main.rs ");
}

fn cmd(name: &str, description: &str, hint: &str, skill: bool) -> aside::Command {
    aside::Command { name: name.into(), description: description.into(), hint: hint.into(), skill }
}

/// Commands rank as Lody ranks them: the name, its prefix, a word's prefix, anywhere, letters in
/// order, then only the description; nothing typed lists the agent's own before its skills.
#[test]
fn slash_commands_rank_by_how_they_match() {
    let mut cmds: Vec<aside::Command> = ["review-branch", "compact", "pr-review", "rename", "context", "mcp:review"]
        .iter()
        .map(|n| cmd(n, "", "", false))
        .collect();
    cmds.push(cmd("simplify", "review changed code", "", true));
    let names = |q: &str| suggest::rank(&cmds, q).iter().map(|c| c.name.clone()).collect::<Vec<_>>();
    assert_eq!(names("review"), ["review-branch", "pr-review", "mcp:review", "simplify"]);
    assert_eq!(names("cpt"), ["compact"]);
    assert_eq!(names("co"), ["compact", "context", "simplify"], "the description (\"code\") comes last");
    assert_eq!(names("").last().unwrap(), "simplify");
    let s = suggest::suggestions(&cmds, "/", Lang::Zh, |_| vec![]);
    assert_eq!(s.rows.iter().filter_map(|r| r.group.clone()).collect::<Vec<_>>(), ["Agent 命令", "Skill"]);
    assert_eq!(s.rows[0].completion, "/review-branch ");
    let files = suggest::suggestions(&cmds, "看 @sr", Lang::Zh, |q| {
        assert_eq!(q, "sr");
        (0..10).map(|i| format!("src/{i}.rs")).collect()
    });
    assert_eq!(files.rows.len(), 8);
    assert_eq!((files.rows[0].label.as_str(), files.rows[0].completion.as_str()), ("@src/0.rs", "看 @src/0.rs "));
}

/// A command picked from the list shows its arguments grey until the person types on.
#[test]
fn a_picked_command_shows_its_arguments() {
    let cmds = [cmd("review", "", "branch name", false)];
    assert_eq!(suggest::argument_hint("/review ", &cmds).as_deref(), Some("branch name"));
    assert_eq!(suggest::argument_hint("/review m", &cmds), None);
    assert_eq!(suggest::argument_hint("/other ", &cmds), None);
    let s = suggest::suggestions(&cmds, "/rev", Lang::En, |_| vec![]);
    assert_eq!(s.rows[0].detail, "Parameters: branch name");
}

/// A preview shows only what the records do not hold yet: a frame older than the newest recorded
/// text is dropped, a newer record clears it, a finished call drops its frame, and the end of the
/// turn drops the rest. Each change names the conversations it touched.
#[test]
fn live_previews_give_way_to_the_record() {
    let mut p = Previews::default();
    let mut log = Log::default();
    add(&mut log, "agent.message", json!({ "text": "前半句" }));
    p.recorded(&log.events[0]);
    let say = |seq: i64, text: &str| json!({ "kind": "say", "actor": "m1", "task": "T1", "seq": seq, "text": text, "thought": "" });
    assert!(p.apply(&say(0, "前半")).is_empty() && p.of("task:T1").is_none(), "made before the record");
    assert_eq!(p.apply(&say(1, "后半句")), ["task:T1", "agent:m1"]);
    assert_eq!(p.of("task:T1").unwrap().text, "后半句");
    assert_eq!(p.of("agent:m1").unwrap().text, "后半句");
    let tool = json!({ "kind": "tool", "actor": "m1", "task": "T1", "seq": 1, "callId": "c9", "title": "pytest",
                       "tool": "execute", "input": "pytest", "output": "1 passed", "status": "in_progress" });
    p.apply(&tool);
    assert_eq!(p.of("task:T1").unwrap().tools["c9"].output, "1 passed");
    add(&mut log, "agent.message", json!({ "text": "后半句" }));
    assert_eq!(p.recorded(&log.events[1]), ["task:T1", "agent:m1"]);
    assert_eq!(p.of("task:T1").unwrap().text, "", "the record took over; the call still runs");
    add(&mut log, "tool.call", json!({ "callId": "c9", "tool": "execute", "status": "completed" }));
    p.recorded(&log.events[2]);
    assert!(p.of("task:T1").is_none());
    assert_eq!(p.frame(&["task:T1".into()]).unwrap(), json!({ "previews": { "task:T1": null } }));
    assert!(p.apply(&tool).is_empty(), "a late frame of a finished call");
    p.apply(&say(3, "还在写"));
    p.apply(&json!({ "kind": "end", "actor": "m1", "task": "T1", "seq": 3 }));
    assert!(p.of("task:T1").is_none());
}

/// The run's documents: deliverables, artifacts and mentions, newest first in each, only those
/// that exist; Janus's own prompts are not read for them.
#[test]
fn run_documents_come_from_deliverables_artifacts_and_mentions() {
    let mut log = Log::default();
    log.add(ev("session.prompt").actor("lead").data(json!({ "prompt": "参考 /w/IGNORED.md" })));
    log.add(ev("board.posted").actor("lead").text("R1 已完成，交付 /w/R1_MARKET_RESEARCH.md（134 行）"));
    log.add(ev("artifact.created").actor("lead").data(json!({ "path": "/j/artifacts/R1-a1-report.md" })));
    log.add(ev("tool.call").actor("lead").data(json!({ "input": "{\"file_path\":\"/w/PRD.md\"}" })));
    log.add(
        ev("agent.message")
            .actor("lead")
            .text("现在创建 `PRD.md` 与README.md，见 docs/设计说明.md 和 README.md/PRD.md"),
    );
    log.add(
        ev("run.delivered").actor("lead").data(json!({ "deliverables": [{ "name": "PRD", "path": "/w/PRD.md" }] })),
    );
    log.add(ev("agent.message").actor("lead").text("改了 src/app.js，没有文档"));
    let mut docs = DocsFold::default();
    for e in &log.events {
        docs.apply(e, e.data.as_ref().unwrap_or(&Value::Null));
    }
    let exist = [
        "/w/R1_MARKET_RESEARCH.md",
        "/j/artifacts/R1-a1-report.md",
        "/w/PRD.md",
        "/w/README.md",
        "/w/docs/设计说明.md",
        "/w/IGNORED.md",
    ];
    let found = docs.existing("/w", None, |p| exist.contains(&p));
    assert_eq!(
        found.iter().map(|d| d.path.as_str()).collect::<Vec<_>>(),
        [
            "/w/PRD.md",
            "/j/artifacts/R1-a1-report.md",
            "/w/README.md",
            "/w/docs/设计说明.md",
            "/w/R1_MARKET_RESEARCH.md"
        ]
    );
    assert!(found[0].source == Source::Deliverable && found[0].seq == 6);
    assert_eq!((found[3].label.as_str(), found[3].name.as_str()), ("docs/设计说明.md", "设计说明.md"));
    assert_eq!(found[1].label, "/j/artifacts/R1-a1-report.md");
}

/// A delivery in a folder without Git delivers no diff: the documents the round changed are its
/// deliverables, relative to the run's folder.
#[test]
fn the_files_a_delivery_changed_are_its_deliverables() {
    let mut log = Log::default();
    log.add(
        ev("run.delivered")
            .data(json!({ "deliverables": [], "changed": ["FINAL_REPORT.md", "data/raw/README.md", "calc.py"] })),
    );
    let mut docs = DocsFold::default();
    docs.apply(&log.events[0], log.events[0].data.as_ref().unwrap());
    let found = docs.existing("/w", None, |_| true);
    assert_eq!(
        found.iter().map(|d| d.path.as_str()).collect::<Vec<_>>(),
        ["/w/FINAL_REPORT.md", "/w/data/raw/README.md"]
    );
    assert!(found.iter().all(|d| d.source == Source::Deliverable));
}

/// Files in a worktree that is gone are found where accepting brought them (FL-19).
#[test]
fn documents_of_a_gone_worktree_are_read_in_the_repository() {
    let cwd = "/Users/me/.janus/work/runs/r1/apps/client";
    let work = Path::new("/Users/me/.janus/work/runs/r1");
    let moved = Moved::of(cwd, "/Users/me/src/app", work, |_| false).unwrap();
    assert_eq!(moved, Moved { from: "/Users/me/.janus/work/runs/r1".into(), to: "/Users/me/src/app".into() });
    let mut log = Log::default();
    log.add(ev("agent.message").actor("lead").text("写好了 docs/manual.md"));
    let mut docs = DocsFold::default();
    docs.apply(&log.events[0], &Value::Null);
    let found = docs.existing(cwd, Some(&moved), |_| true);
    assert_eq!(found[0].path, "/Users/me/src/app/apps/client/docs/manual.md");
    assert!(Moved::of(cwd, "/Users/me/src/app", work, |_| true).is_none(), "a worktree still there is read as is");
}

/// Paths are found in prose the way people write them.
#[test]
fn markdown_paths_are_found_in_prose() {
    assert_eq!(paths_in("生成README.md"), ["生成README.md", "README.md"]);
    assert_eq!(paths_in("见 ~/notes/a.md。"), ["~/notes/a.md"]);
    assert_eq!(paths_in("a.md/b.md"), ["a.md/b.md", "a.md", "b.md"]);
    assert_eq!(paths_in("guide.markdown, x.mdx"), ["guide.markdown"]);
    assert!(paths_in("没有文档").is_empty());
}
