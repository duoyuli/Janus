"""The server going away mid-run: what is kept, what is interrupted, and how work continues."""

import threading

from lib import drawn, wait
from scenarios import scenario, task


@scenario("restart", serial=True)
def restart(ctx, c):
    """服务重启：进行中的 Run 记为中断，看板和痕迹恢复，从中断处继续；已交付的 Run 回放时会话只读，重启后还能继续对话，同时发来的两条消息只恢复一次会话；应用重新连上"""
    t = ctx.team("restart", members=("m1", "m2", "m3"), script={"T1": ["write:a.txt"], "T2": ["slow", "ok"], "T3": ["ok"]})
    run = t.run("跨越重启的目标")
    run.orchestrate(task("T1", t.member("m1"), owns=["a.txt"]), task("T2", t.member("m2")), task("T3", t.member("m3"), deps=["T2"]))
    run.wait_event("task.succeeded", task="T1")
    run.wait_event("attempt.started", task="T2")
    d = ctx.team("restart-done", members=("m1",))
    done = d.run("重启前就交付的目标")
    done.orchestrate(task("D1", d.member("m1")))
    done.wait_event("task.succeeded", task="D1")
    done.finish("交付了")

    ctx.server.stop()
    meta = ctx.server.start()
    c(meta["dataDir"] == ctx.home, "服务在同一个 Janus home 上重新启动")
    run.wait_status("interrupted")
    c(run.has("run.interrupted"), "重启时没结束的 Run 记为中断")
    c(done.status() == "delivered", "已交付的 Run 不受影响")
    c(not ctx.api("GET", f"/api/runs/{run.id}")["live"], "中断的 Run 不在内存中运行")
    code, _ = run.act_error("pause")
    c(code == 409, "中断的 Run 不能执行要求它正在运行的操作（409）")
    ctx.ui(c, run.id, lambda r, s: r["status"] == "interrupted", "应用重新连上服务，显示 Run 已中断", pages=("board",))

    run.act("resumeRun")
    run.wait_event("run.recovered")
    c(True, "从中断处继续")
    wait(lambda: len([u for u in t.fake.urls(run.id) if u.endswith("/lead")]) >= 2, "负责人会话以新的看板地址重新打开")
    c(True, "恢复后的负责人会话拿到新的看板地址（旧令牌作废）")
    board = run.read_board(lambda b: "[完成] T3" in b, timeout=90)
    c("[完成] T2" in board, "中断的 T2 重新执行并完成，下游 T3 随后完成")
    c(not [e for e in run.has("attempt.started") if e.get("task") == "T1"][1:], "已完成的 T1 不重跑")
    lead = run.lead()
    detail, _ = lead.call("task_detail", {"task": "T1"})
    c("a.txt" in detail, "重启后 T1 改过的文件（痕迹）仍在")
    run.finish("重启后也完成了")
    c(run.status() == "delivered", "恢复的 Run 交付")

    # Replaying it shows the lead's session read-only: a draft typed there stays and is not sent.
    if ctx.ui(c, done.id, lambda r, s: r["status"] == "delivered", "重启前交付的 Run 在应用中已交付"):
        shot = f"{done.id}-replay-draft"
        ctx.app.do(f"open run:{done.id}", "page replay:3", "draft lead 回放时写的草稿", f"shot {shot}")
        c(drawn(ctx.app.picture(shot)), f"应用：回放中负责人会话显示「回放中」、输入框只读、草稿保留（{shot}.png）")
        ctx.app.do("page board")
        c(not done.has("user.message"), "回放中写的草稿没有发出去")

    # A run delivered before the restart takes a message: the lead's session reopens. Two sent at
    # once bring it back once, and both reach the lead.
    texts = ("重启后还记得吗", "再补一句")
    sends = [threading.Thread(target=done.act, args=("message",), kwargs={"agent": "lead", "text": t}) for t in texts]
    for s in sends:
        s.start()
    for s in sends:
        s.join()
    said = wait(lambda: done.has("user.message")[1:] and done.has("user.message"), "两条消息都记下")
    sent = max(e["seq"] for e in said)
    wait(lambda: [e for e in done.events() if e["type"] == "agent.message" and e["actor"] == "lead" and e["seq"] > sent], "负责人回复")
    c(d.fake.calls().count("load ") == 1, "两条消息同时发给只有事件记录的历史 Run，只加载一次原会话（session/load）")

    def reached(m):
        mid = m["data"]["id"]
        return any(e["type"] == "message.routed" and e["data"].get("id") == mid and e["data"].get("delivery") in ("steer", "prompt")
                   or e["type"] == "message.delivered" and mid in e["data"].get("ids", []) for e in done.events())
    wait(lambda: all(reached(m) for m in said), "两条消息都送到负责人")
    c(True, "同时发来的两条消息都送到恢复的负责人会话")
    ctx.ui(c, run.id, lambda r, s: r["status"] == "delivered" and all(x["status"] == "succeeded" for x in r["tasks"]),
           "恢复后的 Run 在应用中已交付，任务都已完成", pages=("board", "audit"))
