"""Things going wrong: failed attempts, unmet criteria, verifiers that reject, and the lead's choices."""

from lib import wait
from scenarios import scenario, task


@scenario("failures")
def failures(ctx, c):
    """失败与处理：尝试失败后回到原会话重试、重试后下游恢复、没通过验收由负责人接受、取消连带下游、验证者判定不通过不能放行而是回到原会话返工并复审、返工时提醒基于旧结果的下游、不合法的证据方法"""
    t = ctx.team("failures", members=("m1", "m2", "m3", "m4", "m5", "m6", "m7", "m8"), script={
        "T1": ["fail", "fail", "ok"], "T2": ["ok"], "T3": ["unmet", "unmet"], "T4": ["fail", "fail"],
        "T5": ["ok"], "T6": ["handoff", "ok"], "V6": ["reject", "approve"], "D6": ["slow", "ok"], "T7": ["badmethod", "badmethod"]})
    m = t.member
    run = t.run("会出问题的目标")
    lead = run.lead()
    run.orchestrate(
        task("T1", m("m1")), task("T2", m("m2"), deps=["T1"]),
        task("T3", m("m3")),
        task("T4", m("m4")), task("T5", m("m5"), deps=["T4"]),
        task("T6", m("m6"), owns=["api.rs"]), task("V6", m("m7"), verifies="T6"), task("D6", m("m8"), deps=["T6"]),
        task("T7", m("m3"), deps=["T3"]))
    # T2 is blocked right after T1's last failure, in the next board entry: wait for it too.
    board = run.read_board(lambda b: all(x in b for x in ("[失败] T1", "[无法开始] T2", "[失败] T4", "[没通过验收] T3", "[验证未通过] V6")), timeout=120)
    c("[尝试失败] T1" in board, "第一次失败后自动重试（[尝试失败]）")
    t1 = [e["data"] for e in run.has("attempt.started", task="T1")]
    c(len(t1) >= 2 and t1[1]["session"] == t1[0]["session"] and t1[1]["resumed"] and "上一次尝试因为" in t1[1]["prompt"],
      "自动重试回到断掉的会话，告诉它上次为什么中断，不重发任务")
    c("[无法开始] T2" in board, "依赖失败的下游标为无法开始")
    failed = run.has("task.failed", task="T3")
    c(failed and failed[-1]["data"]["reason"] == "verification", "没达到验收标准：原会话补一次仍不行，交给负责人")
    c(run.has("verification.failed", task="T3"), "核对结果记为 verification.failed")

    # The lead's choices.
    text, err = lead.call("retry", {"task": "T1", "session": "new"})
    c(not err, "负责人重试 T1")
    run.wait_event("task.succeeded", task="T2", timeout=60)
    c(True, "T1 重试成功后，被它阻塞的 T2 自动继续并完成")
    text, err = lead.call("accept", {"task": "T3", "reason": "人工核对过，不影响目标"})
    c(not err and run.has("task.succeeded", task="T3", actor="lead"), "负责人接受没通过验收的 T3，未达到的部分记为风险")
    text, err = lead.call("cancel", {"tasks": ["T4"], "reason": "这部分不做了，写进交付风险"})
    skipped = {e["task"] for e in run.has("task.skipped_by_decision")}
    c(not err and {"T4", "T5"} <= skipped, f"取消 T4，依赖它的 T5 一并取消（{sorted(skipped)}）")
    text, err = lead.call("accept", {"task": "V6", "reason": "就这样吧"})
    c(err, f"验证者判定不通过时，负责人不能用 accept 放行（{text[:60]}）")
    verdict = run.has("task.succeeded", task="V6")
    c(verdict and verdict[-1]["data"].get("verdict") == "rejected", "V6 的判定 rejected 记在任务结论里")
    # The lead sends T6 back into its own session on V6's findings; V6 checks the rework again.
    text, err = lead.call("retry", {"task": "T6", "from": "V6"})
    c(err and "session" in text, f"返工时负责人必须选择会话（{text[:40]}）")
    text, err = lead.call("retry", {"task": "T6", "session": "continue", "from": "V6", "note": "空输入按 ParseError 处理"})
    c(not err and "V6 重新验证" in text, f"负责人让 T6 按 V6 的意见在原会话返工（{text[:60]}）")
    c("D6 正在基于 T6 的旧结果执行" in text, "返工时告诉负责人：D6 正在基于 T6 的旧结果执行")
    run.wait_event("task.succeeded", task="D6", timeout=60)
    c(any("上游子任务 T6 正在返工" in e["data"].get("text", "") for e in run.has("user.message", task="D6")),
      "正在执行的下游 D6 在会话中收到「上游在返工」的通知")
    wait(lambda: len(run.has("task.succeeded", task="V6")) >= 2, "V6 复审 T6 的返工", 60)
    t6 = [e["data"] for e in run.has("attempt.started", task="T6")]
    v6 = [e["data"] for e in run.has("attempt.started", task="V6")]
    c(len(t6) == 2 and t6[1]["session"] == t6[0]["session"] and t6[1].get("continuation", {}).get("loaded"),
      "T6 的返工回到它原来的会话，没有新建子任务")
    c("运行 cargo test 复现空输入崩溃" in t6[-1]["prompt"] and "空输入按 ParseError 处理" in t6[-1]["prompt"],
      "V6 的意见原样交给执行者，连同负责人的说明")
    c(len(v6) == 2 and v6[1]["session"] == v6[0]["session"] and v6[1]["reopen"]["reason"] == "target.reworked",
      "返工完成后 V6 在自己的会话中复审")
    c(run.has("task.succeeded", task="V6")[-1]["data"].get("verdict") == "approved", "复审通过，V6 的判定变为 approved")
    run.wait_event("task.failed", task="T7", timeout=60)
    c("vibes" in str(run.has("verification.failed", task="T7")), "不合法的证据方法（vibes）原样记录且核对失败")
    delivered = run.finish("能做的都做了，T4 这部分没有做，见风险")
    c(delivered["data"].get("risks") or delivered["data"].get("evidence"), "交付带着风险和证据")
    acceptance = {a["task"]: a for a in delivered["data"]["acceptance"]}
    c(acceptance["T6"]["unverified"] is False and acceptance["V6"]["unverified"] is False,
      "改了文件的 T6 有独立验证，交付不标「没有独立验证」")
    ctx.ui(c, run.id, lambda r, s: {x["id"]: x["status"] for x in r["tasks"]} == {
        "T1": "succeeded", "T2": "succeeded", "T3": "succeeded", "T4": "skipped_by_decision", "T5": "skipped_by_decision",
        "T6": "succeeded", "V6": "succeeded", "D6": "succeeded", "T7": "failed"} and next(x for x in r["tasks"] if x["id"] == "V6")["verdict"] == "approved",
        "每个任务的状态与服务一致，V6 复审后显示通过", pages=("board", "fold all open", "task:T3", "task:T6", "task:V6", "team"))
