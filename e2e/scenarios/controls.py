"""The person's run operations: pause, interrupt, take over, switch controls, budget, cancel, delete."""

from lib import wait
from scenarios import scenario, task


@scenario("controls")
def controls(ctx, c):
    """Run 操作：暂停与继续、打断后发消息、接管与交还（负责人在看板上读到用户介入）、切换控制项、追加预算、状态冲突、取消、删除"""
    t = ctx.team("controls", members=("m1", "m2", "m3"), script={"T1": ["slow", "ok"], "T2": ["slow"], "T3": ["ok"]})
    run = t.run("可以被操作的目标")
    run.orchestrate(task("T1", t.member("m1")), task("T2", t.member("m2")), task("T3", t.member("m3"), deps=["T1"]))
    run.wait_event("attempt.started", task="T1")
    run.wait_event("attempt.started", task="T2")

    run.act("pause")
    run.wait_event("run.paused")
    c(run.status() == "running", "暂停时 Run 仍是执行中，进行中的尝试继续")
    run.act("resume")
    run.wait_event("run.resumed")
    c(True, "继续")
    code, _ = run.act_error("resumeRun")
    c(code == 409, "Run 正在运行时不能「从中断处继续」（409）")
    code, _ = run.act_error("approve", key="nope", task="T1", allow=True)
    c(code == 400, "没有等待中的决定时审批返回 400")

    # Interrupt a turn, then talk to the session: the next prompt finishes the task.
    run.act("interrupt", task="T1")
    run.wait_event("session.interrupted", task="T1")
    c(True, "打断 T1 当前的一轮")
    wait(lambda: any("人工接管" in e["text"] for e in run.has("agent.status", task="T1")), "T1 转为人工接管")
    c(True, "打断后转为人工接管")
    run.act("message", task="T1", text="按原来的说明继续")
    wait(lambda: len(run.has("agent.message", task="T1")) >= 1, "T1 回复用户的消息")
    c(not run.has("task.succeeded", task="T1"), "人工接管时 Agent 的回复不自动结束任务")
    run.act("finish", task="T1")
    run.wait_event("task.succeeded", task="T1", timeout=60)
    c(run.wait_event("task.succeeded", task="T3", timeout=60), "用户以当前结果结束 T1，下游 T3 随后完成")

    # Take a session over and give it back; switch its controls.
    run.act("takeover", task="T2")
    run.wait_event("control.taken", task="T2")
    run.act("release", task="T2")
    run.wait_event("control.released", task="T2")
    c(True, "接管并交还 T2 的会话")
    # The lead runs the board, so the person's steps into collaborators' sessions reach it.
    board = run.read_board(lambda b: "交还给" in b, timeout=30)
    c(all(x in board for x in ("[用户介入] T1", "用户对", "按原来的说明继续", "以当前结果结束", "[用户介入] T2", "接管了", "交还给")),
      "负责人在看板上读到用户对 T1、T2 会话的介入（打断、消息、结束、接管、交还）")
    run.act("control", task="T2", id="mode", value="agent")
    switched = run.wait_event("session.control", task="T2", data__id="mode")
    c(switched["data"]["value"] == "agent" and switched["data"].get("by") == "user", "用户在会话中切换权限模式")
    wait(lambda: "set mode \"agent\"" in t.fake.calls(), "Agent 收到 set_config_option")
    code, _ = run.act_error("control", task="T2", id="mode", value="")
    c(code == 400, "空的控制项值被拒绝")
    run.act("extendBudget", minutes=30)
    c(run.wait_event("run.budget_extended")["data"].get("minutes") == 30, "追加 30 分钟预算")
    run.act("extendBudget", minutes=5000)
    c(run.has("run.budget_extended")[-1]["data"].get("minutes") == 1440, "超出范围的预算按上限 1440 分钟追加")

    code, _ = ctx.api_error("DELETE", f"/api/runs/{run.id}")
    c(code == 409, "进行中的 Run 不能删除（409）")
    if ctx.app:
        ctx.ui(c, run.id, lambda r, s: r["status"] == "running" and next(x for x in r["tasks"] if x["id"] == "T2")["status"] == "running",
               "运行中：T2 执行中", pages=("board", "task:T2"))
    run.act("cancel")
    run.wait_status("cancelled")
    c(True, "取消 Run，终止所有会话")
    ledger = ctx.api("GET", f"/api/runs/{run.id}/usage")
    t2 = [a for a in ledger["attempts"] if a.get("task") == "T2"]
    c(t2 and t2[-1]["state"] == "cancelled", f"用量账本把进行中的 T2 尝试记为已取消：{[a['state'] for a in t2]}")
    code, _ = run.act_error("cancel")
    c(code in (400, 409), "已取消的 Run 不能再取消")
    ctx.ui(c, run.id, lambda r, s: r["status"] == "cancelled", "Run 显示已停止", pages=("board",))
    ctx.api("DELETE", f"/api/runs/{run.id}")
    code, _ = ctx.api_error("GET", f"/api/runs/{run.id}")
    c(code == 404, "删除后 Run 不存在（404）")
    c(run.id not in [r["id"] for r in ctx.api("GET", "/api/runs")["runs"]], "删除后不在 Run 列表中")
    import os
    c(not os.path.exists(os.path.join(ctx.home, "artifacts", run.id)), "删除后 Janus home 中这个 Run 的产物目录也删掉了")
    if ctx.app:
        ctx.app.until(lambda s: run.id not in [x["id"] for x in s["runs"]], "应用的 Run 列表不再有它", "reload", f"open home:{t.ws['id']}")
        c(True, "应用：删除的 Run 不再出现")
