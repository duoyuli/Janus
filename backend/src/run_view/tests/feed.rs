//! The run page as a group: which posts the group shows and where, each task's topic, what waits
//! on the person, and who is at work.

use super::*;
use crate::run_view::feed::{Post, PostKind};
use crate::run_view::sessions::agent_key;
use crate::run_view::turns::{Row, post_id};

/// The lead's conversation as the group shows it: each row by kind, posts by theirs.
fn group(v: &RunView) -> Vec<String> {
    v.sessions[&agent_key("lead")]
        .rows
        .iter()
        .map(|r| match r {
            Row::Post { post, .. } => format!("post:{}", post.kind.as_str()),
            Row::Section { seq, .. } | Row::Item { seq, .. } => format!("event:{seq}"),
            Row::Head { .. } => "head".into(),
            Row::Activity { .. } => "activity".into(),
            Row::Text { .. } => "text".into(),
            Row::Edits { .. } => "edits".into(),
        })
        .collect()
}

fn posts(v: &RunView) -> Vec<&Post> {
    v.sessions[&agent_key("lead")]
        .rows
        .iter()
        .filter_map(|r| match r {
            Row::Post { post, .. } => Some(post.as_ref()),
            _ => None,
        })
        .collect()
}

/// The view as a client drawing the group asks for it.
fn view(log: &Log) -> RunView {
    view_in(log, Lang::Zh)
}

fn view_in(log: &Log, lang: Lang) -> RunView {
    fold(&log.events).view(true, false, &Words::of(lang, &json!({})), Layout { detail: false, group: true })
}

fn topic(v: &RunView, task: &str) -> Vec<String> {
    v.feed.topics.get(task).into_iter().flatten().map(|p| p.kind.as_str().to_string()).collect()
}

#[test]
fn the_group_is_the_leads_conversation_with_the_posts_in_their_place() {
    let mut log = goal_run();
    // The lead says why before its turn ends; a board push repeats what the posts say.
    log.add(ev("agent.message").actor("lead").data(json!({ "text": "先派两件。" })));
    log.add(
        ev("board.posted").actor("researcher").task("T1").data(json!({ "kind": "progress", "text": "读完了调用方" })),
    );
    log.add(ev("session.prompt").to("lead").data(json!({ "channel": "board", "prompt": "[Janus 看板推送] …" })));
    let v = view(&log);
    // run.created, then the dispatch and the start at their seqs, then the lead's words.
    assert_eq!(group(&v), ["event:1", "post:dispatch", "post:start", "head", "text"]);
    let p = posts(&v);
    assert_eq!(p[0].title, "派了 2 件事");
    assert_eq!(
        p[0].tasks.iter().map(|t| (t.id.as_str(), t.scope.as_str())).collect::<Vec<_>>(),
        [("T1", ""), ("T2", "依赖 T1")]
    );
    assert_eq!(p[1].text, "researcher 开始 T1");
    // Progress stays in the task's topic; the push is left out of the group.
    assert_eq!(topic(&v, "T1"), ["start", "note"]);
    assert!(!group(&v).contains(&"event:7".to_string()), "the board push is not in the group");
}

#[test]
fn a_question_stays_in_its_topic_until_the_lead_passes_it_on() {
    let mut log = goal_run();
    log.add(
        ev("question.asked")
            .actor("researcher")
            .task("T1")
            .data(json!({ "key": "q1", "question": "上限多少？", "options": ["1 万", "不限"] })),
    );
    let v = view(&log);
    assert_eq!(topic(&v, "T1"), ["start", "question"]);
    assert!(posts(&v).iter().all(|p| p.kind != PostKind::Question), "the lead answers it: not in the group");
    assert!(v.feed.mentions.is_empty());

    log.add(ev("question.escalated").actor("lead").task("T1").data(json!({ "key": "q1", "reason": "改接口行为" })));
    let v = view(&log);
    let ask = posts(&v).into_iter().find(|p| p.kind == PostKind::Ask).unwrap();
    assert_eq!((ask.text.as_str(), ask.key.as_str(), ask.you), ("上限多少？", "q1", true));
    assert_eq!(v.feed.mentions, [post_id(ask.seq)], "「@你」 jumps to it");
    assert_eq!(topic(&v, "T1"), ["start", "question", "ask"]);

    log.add(
        ev("question.answered")
            .actor("user")
            .to("researcher")
            .task("T1")
            .data(json!({ "key": "q1", "answer": "1 万" })),
    );
    let v = view(&log);
    let ask = posts(&v).into_iter().find(|p| p.kind == PostKind::Ask).unwrap();
    assert!(!ask.you);
    assert_eq!(ask.settled, "回答：1 万");
    assert!(v.feed.mentions.is_empty());
    assert_eq!(topic(&v, "T1"), ["start", "question", "ask", "message"], "the answer is said in the topic");
}

#[test]
fn the_leads_own_decision_waits_in_the_group() {
    let mut log = goal_run();
    log.add(
        ev("question.escalated")
            .actor("lead")
            .data(json!({ "key": "d1", "question": "要不要发布？", "options": ["发布", "再等等"] })),
    );
    let v = view(&log);
    let ask = posts(&v).into_iter().find(|p| p.kind == PostKind::Ask).unwrap();
    assert_eq!((ask.text.as_str(), ask.task.as_deref(), ask.you), ("要不要发布？", None, true));
    assert_eq!(v.feed.mentions, [post_id(ask.seq)]);
}

#[test]
fn a_collaborators_approval_waits_in_the_group_and_says_how_it_ended() {
    let mut log = goal_run();
    log.add(
        ev("approval.requested")
            .actor("researcher")
            .task("T1")
            .attempt(1)
            .text("pip install x")
            .data(json!({ "key": "a1" })),
    );
    let ask = |v: &RunView| posts(v).into_iter().find(|p| p.kind == PostKind::Ask).cloned().unwrap();
    let v = view(&log);
    assert_eq!((ask(&v).title.as_str(), ask(&v).text.as_str(), ask(&v).you), ("请你批准", "pip install x", true));
    log.add(ev("approval.granted").actor("user").to("researcher").task("T1").data(json!({ "key": "a1" })));
    assert_eq!(ask(&view(&log)).settled, "已批准");
    // The lead's own approval is in its conversation already.
    log.add(ev("approval.requested").actor("lead").text("rm -rf build").data(json!({ "key": "a2" })));
    assert_eq!(posts(&view(&log)).iter().filter(|p| p.kind == PostKind::Ask).count(), 1);
}

#[test]
fn results_handoffs_rework_and_conflicts_are_posts_everyone_sees() {
    let mut log = goal_run();
    log.add(
        ev("task.conflict")
            .actor("janus")
            .to("lead")
            .task("T1")
            .data(json!({ "other": "T2", "paths": ["a.py"], "owned": true })),
    );
    log.add(ev("task.succeeded").task("T1").attempt(1).data(json!({
        "summary": "列好了", "handoff": "看 calls.md",
        "checks": [{ "criterion": "列出调用方", "passed": true, "method": "inspection" }],
    })));
    log.add(ev("context.handoff").task("T2").data(json!({ "from": "T1" })));
    log.add(ev("attempt.started").to("implementer").task("T2").attempt(1).data(json!({ "session": "s2" })));
    log.add(ev("task.reopened").actor("lead").task("T2").data(json!({ "reason": "lead.retry", "note": "补测试" })));
    let v = view(&log);
    let p = posts(&v);
    let kinds: Vec<&str> = p.iter().map(|p| p.kind.as_str()).collect();
    assert_eq!(kinds, ["dispatch", "start", "conflict", "result", "start", "rework"]);
    assert_eq!(p[2].text, "T1 改了 T2 负责的 a.py，已提醒它别再改");
    assert_eq!(
        (p[3].title.as_str(), p[3].text.as_str(), p[3].meta.as_str()),
        ("交了结果", "列好了", "验收 1/1 · 检查 1 条")
    );
    assert_eq!(p[3].lines, ["交接给下游：看 calls.md"]);
    assert_eq!(p[4].text, "T1 的结果交给了 T2 · implementer 开始 T2");
    assert_eq!((p[5].title.as_str(), p[5].text.as_str(), p[5].by.as_str()), ("返工 T2", "补测试", "lead"));
    // A conflict is in both tasks' topics.
    assert_eq!(topic(&v, "T2"), ["conflict", "start", "rework"]);

    let en = view_in(&log, Lang::En);
    let p = posts(&en);
    assert_eq!(p[3].meta, "Criteria 1/1 · 1 inspection");
    assert_eq!(p[4].text, "T1 handed over to T2 · implementer starts T2");
}

#[test]
fn the_persons_message_to_a_collaborator_is_in_the_group_and_the_leads_only_in_the_topic() {
    let mut log = goal_run();
    log.add(
        ev("user.message")
            .actor("user")
            .to("researcher")
            .task("T1")
            .data(json!({ "id": "m1", "text": "顺便看下 b.py" })),
    );
    log.add(
        ev("user.message").actor("lead").to("researcher").task("T1").data(json!({ "id": "m2", "text": "先别改文档" })),
    );
    let v = view(&log);
    let msgs: Vec<(&str, &str)> =
        posts(&v).iter().filter(|p| p.kind == PostKind::Message).map(|p| (p.by.as_str(), p.text.as_str())).collect();
    assert_eq!(msgs, [("user", "顺便看下 b.py")]);
    assert_eq!(topic(&v, "T1"), ["start", "message", "message"]);
}

#[test]
fn each_delivery_is_a_post_and_the_one_awaiting_acceptance_mentions_the_person() {
    let mut log = goal_run();
    log.add(ev("task.succeeded").task("T1").attempt(1).data(json!({ "summary": "好了" })));
    log.add(ev("run.delivered").actor("lead").data(json!({ "summary": "交付" })));
    let v = view(&log);
    let d = posts(&v).into_iter().find(|p| p.kind == PostKind::Delivery).unwrap();
    assert_eq!((d.n, d.you, d.text.as_str()), (1, true, "交付"));
    assert_eq!(v.feed.mentions, [post_id(d.seq)]);
    log.add(ev("risk.accepted").actor("user"));
    let v = view(&log);
    assert!(!posts(&v).into_iter().find(|p| p.kind == PostKind::Delivery).unwrap().you);
}

#[test]
fn who_is_at_work_says_the_step_they_are_on() {
    let mut log = goal_run();
    assert_eq!(
        view(&log).feed.typing.iter().map(|t| (t.agent.as_str(), t.text.as_str())).collect::<Vec<_>>(),
        [("researcher", "在开工")]
    );
    log.add(
        ev("tool.started")
            .actor("researcher")
            .task("T1")
            .attempt(1)
            .text("pytest -q")
            .data(json!({ "callId": "c1", "tool": "execute", "live": true })),
    );
    let t = &view(&log).feed.typing[0];
    assert_eq!((t.task.as_str(), t.text.as_str()), ("T1", "正在运行 pytest -q"));
    // A turn that ended waits for the person: nobody is typing.
    log.add(ev("agent.status").actor("researcher").task("T1").data(json!({ "turn": "idle" })));
    assert!(view(&log).feed.typing.is_empty());
}

#[test]
fn without_the_group_layout_the_conversation_is_as_it_was() {
    let mut log = goal_run();
    log.add(ev("session.prompt").to("lead").data(json!({ "channel": "board", "prompt": "[Janus 看板推送] …" })));
    let v = log.view();
    assert!(posts(&v).is_empty(), "no posts in the lead's rows");
    assert!(group(&v).contains(&"event:5".to_string()), "the board push stays");
    assert_eq!(topic(&v, "T1"), ["start"], "the topics are served either way");
}

#[test]
fn a_run_without_tasks_keeps_its_conversation_as_it_was() {
    let mut log = Log::default();
    log.add(ev("run.created").actor("user").data(json!({ "kind": "chat", "goal": "" })));
    log.add(ev("agent.message").actor("lead").data(json!({ "text": "你好" })));
    let v = view(&log);
    assert!(posts(&v).is_empty());
    assert!(v.feed.topics.is_empty() && v.feed.typing.is_empty());
}
