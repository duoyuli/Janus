"""Things going wrong: failed attempts, unmet criteria, verifiers that reject, and the lead's choices."""

from scenarios import scenario, task


@scenario("failures")
def failures(ctx, c):
    """失败与处理：重试后下游恢复、没通过验收由负责人接受、取消连带下游、验证者判定不通过不能放行、不合法的证据方法"""
    t = ctx.team("failures", members=("m1", "m2", "m3", "m4", "m5", "m6", "m7"), script={
        "T1": ["fail", "fail", "ok"], "T2": ["ok"], "T3": ["unmet", "unmet"], "T4": ["fail", "fail"],
        "T5": ["ok"], "T6": ["handoff"], "V6": ["reject"], "T7": ["badmethod", "badmethod"]})
    m = t.member
    run = t.run("会出问题的目标")
    lead = run.lead()
    run.orchestrate(
        task("T1", m("m1")), task("T2", m("m2"), deps=["T1"]),
        task("T3", m("m3")),
        task("T4", m("m4")), task("T5", m("m5"), deps=["T4"]),
        task("T6", m("m6"), owns=["api.rs"]), task("V6", m("m7"), verifies="T6"),
        task("T7", m("m3"), deps=["T3"]))
    board = run.read_board(lambda b: all(x in b for x in ("[失败] T1", "[失败] T4", "[没通过验收] T3", "[验证未通过] V6")), timeout=120)
    c("[尝试失败] T1" in board, "第一次失败后自动重试（[尝试失败]）")
    c("[无法开始] T2" in board, "依赖失败的下游标为无法开始")
    failed = run.has("task.failed", task="T3")
    c(failed and failed[-1]["data"]["reason"] == "verification", "没达到验收标准：原会话补一次仍不行，交给负责人")
    c(run.has("verification.failed", task="T3"), "核对结果记为 verification.failed")

    # The lead's choices.
    text, err = lead.call("retry", {"task": "T1"})
    c(not err, "负责人重试 T1")
    run.wait_event("task.succeeded", task="T2", timeout=60)
    c(True, "T1 重试成功后，被它阻塞的 T2 自动继续并完成")
    text, err = lead.call("accept", {"task": "T3", "reason": "人工核对过，不影响目标"})
    c(not err and run.has("task.succeeded", task="T3", actor="lead"), "负责人接受没通过验收的 T3，未达到的部分记为风险")
    text, err = lead.call("cancel", {"tasks": ["T4"], "reason": "改由负责人自己完成"})
    skipped = {e["task"] for e in run.has("task.skipped_by_decision")}
    c(not err and {"T4", "T5"} <= skipped, f"取消 T4，依赖它的 T5 一并取消（{sorted(skipped)}）")
    text, err = lead.call("accept", {"task": "V6", "reason": "就这样吧"})
    c(err, f"验证者判定不通过时，负责人不能用 accept 放行（{text[:60]}）")
    verdict = run.has("task.succeeded", task="V6")
    c(verdict and verdict[-1]["data"].get("verdict") == "rejected", "V6 的判定 rejected 记在任务结论里")
    run.wait_event("task.failed", task="T7", timeout=60)
    c("vibes" in str(run.has("verification.failed", task="T7")), "不合法的证据方法（vibes）原样记录且核对失败")
    delivered = run.finish("能做的都做了，T4 改由我处理")
    c(delivered["data"].get("risks") or delivered["data"].get("evidence"), "交付带着风险和证据")
    ctx.ui(c, run.id, lambda r, s: {x["id"]: x["status"] for x in r["tasks"]} == {
        "T1": "succeeded", "T2": "succeeded", "T3": "succeeded", "T4": "skipped_by_decision", "T5": "skipped_by_decision",
        "T6": "succeeded", "V6": "succeeded", "T7": "failed"} and next(x for x in r["tasks"] if x["id"] == "V6")["verdict"] == "rejected",
        "每个任务的状态与服务一致，V6 显示判定不通过", pages=("board", "fold all open", "task:T3", "task:V6", "team"))
