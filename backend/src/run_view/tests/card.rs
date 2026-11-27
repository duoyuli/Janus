//! The run card's status and the team panel's rows.

use super::*;
use crate::run_view::card::Tone;

fn team() -> Value {
    json!({ "team": [
        { "id": "lead", "name": "负责人", "lead": true },
        { "id": "researcher", "name": "研究员" },
        { "id": "implementer", "name": "实现者" },
        { "id": "idle", "name": "闲着的" },
    ]})
}

#[test]
fn a_goal_is_titled_by_its_first_line_without_markdown() {
    assert_eq!(goal_title("\n  ## 给 **divide** 加 `保护`，见 [文档](docs/a.md)\n细节"), "给 divide 加 保护，见 文档");
    assert_eq!(goal_title(""), "");
    assert_eq!(plain("[不是链接](有 空格) 和 [x]"), "[不是链接](有 空格) 和 [x]");
}

#[test]
fn the_card_says_the_first_state_that_applies() {
    let mut log = goal_run();
    let v = log.view();
    assert_eq!((v.capsule.text.as_str(), v.capsule.tone), ("团队干活中", Tone::Blue));
    log.add(ev("approval.requested").actor("researcher").task("T1").attempt(1).data(json!({ "key": "a-1" })));
    assert_eq!(log.view().capsule.text, "1 件待处理");
    log.add(ev("run.paused").actor("user"));
    assert_eq!(log.view().capsule.text, "已暂停", "paused comes before what waits");
    let en = log.view_in(Lang::En, true, &team());
    assert_eq!((en.capsule.text.as_str(), en.capsule.tone), ("Paused", Tone::Gray));
    log.add(ev("run.interrupted"));
    assert_eq!(log.view().capsule.text, "已中断");
}

#[test]
fn each_member_says_what_it_is_on_and_how_its_work_went() {
    let mut log = goal_run();
    let rows = |log: &Log| log.view_in(Lang::Zh, true, &team()).members;
    let r = rows(&log);
    assert_eq!(r.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["lead", "researcher", "implementer", "idle"]);
    assert_eq!(r[0].line, "按依赖调度子任务");
    assert_eq!(r[1].line, "T1 调研");
    assert_eq!(r[1].phase, AgentPhase::Thinking);
    assert_eq!(r[2].line, "下一步 T2 · 等待 T1");
    assert_eq!(r[2].phase, AgentPhase::Idle, "no task in progress is idle");
    assert_eq!(r[3].line, "负责人没有给它派子任务");
    assert!(r[1].stats.is_none(), "nothing ended yet");

    log.add(ev("verification.failed").task("T1").attempt(1));
    log.add(ev("task.succeeded").task("T1").attempt(1).data(json!({ "summary": "好了", "checks": [] })));
    let r = rows(&log);
    assert_eq!(r[1].line, "派给它的子任务都做完了");
    assert_eq!(r[1].stats.as_deref(), Some("本次 0/1 首次通过 · 验证没通过 1"));
    assert_eq!(r[2].line, "下一步 T2 · 等待 调度");
}
