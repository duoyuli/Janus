"""The dispatch queue: goal runs wait for a free slot, in order; a queued run only starts or is cancelled."""

from lib import wait
from scenarios import scenario, task


@scenario("queue", own=True)
def queue(ctx, c):
    """派发队列：同时在做的任务满了时新目标排队，按先后开始；排队的任务只能马上开工或取消；暂停的任务让出位置；应用显示排位；队列跨工作区，服务重启后在做的目标先自动继续、排队的目标等它让出位置"""
    # A server of its own with one slot.
    with ctx.private("queue", JANUS_MAX_RUNNING_RUNS="1") as ctx:
        t = ctx.team("queue", members=("m1",), script={"Q1": ["slow"]})

        def rows(s):
            return {r["id"]: r for r in (s.get("histories") or {}).get(t.ws["id"], [])}

        first = t.run("占着位置的目标")
        first.orchestrate(task("Q1", t.member("m1")))
        first.wait_event("attempt.started", task="Q1")
        c(first.status() == "running", "第一个目标拿到唯一的位置，直接开始")

        second = t.run("排在第一位的目标")
        third = t.run("排在第二位的目标")
        c(second.status() == "queued" and third.status() == "queued", "位置满了，后两个目标排队")
        c([e["type"] for e in second.events()] == ["run.queued"], "排队的目标只有 run.queued，没有开始任何东西")
        c(second.detail()["run"].get("queuePosition") == 1 and third.detail()["run"].get("queuePosition") == 2, "详情按先后给出排位 1、2")
        listed = {r["id"]: r for r in ctx.api("GET", f"/api/runs?workspace={t.ws['id']}")["runs"]}
        c(listed[third.id].get("queuePosition") == 2, "列表里的行也带排位")
        ctx.ui(c, third.id, lambda r, s: r["status"] == "queued" and r["queuePosition"] == 2 and r["goal"] == "排在第二位的目标",
               "排队的任务显示目标和排位 2", pages=("board",))
        if ctx.app:
            ctx.app.until(lambda s: rows(s).get(third.id, {}).get("label") in ("排队中 · 第 2 位", "Queued · #2"), "工作区的任务记录写出排位",
                          "reload", f"open home:{t.ws['id']}")
            ctx.app.do("shot queue-home")
            c(True, "应用：工作区页的任务记录写「排队中 · 第 2 位」（queue-home.png）")

        code, _ = second.act_error("message", agent="lead", text="你好")
        c(code == 409, "排队的目标不能发消息（409）")
        third.act("cancel")
        third.wait_status("cancelled")
        c(third.has("run.cancelled"), "取消排队的目标，它不会再开始")

        # Pausing the working run gives its slot to the next one.
        first.act("pause")
        second.wait_status("running")
        c(second.has("run.created"), "暂停第一个目标后，排在第一位的目标自动开始")
        first.act("resume")

        fourth = t.run("不等位置的目标")
        c(fourth.status() == "queued", "两个目标在做时新目标排队")
        ctx.ui(c, fourth.id, lambda r, s: r["status"] == "queued" and r["queuePosition"] == 1, "排队的任务排在第一位")
        fourth.act("start")
        fourth.wait_status("running")
        c(True, "「马上开工」越过位置开始排队的目标")
        if ctx.app:
            ctx.app.until(lambda s: rows(s).get(third.id, {}).get("status") == "cancelled", "工作区的任务记录读到取消的目标",
                          "reload", f"open home:{t.ws['id']}")
            c(True, "应用：工作区页列出排队后取消的目标")
        for r in (first, second, fourth):
            r.act("cancel")
            r.wait_status("cancelled")

        # One queue for every workspace, and it survives the service stopping.
        u = ctx.team("queue-other", members=("m1",), script={"Q2": ["slow"]})
        blocker = u.run("另一个工作区占着位置的目标")
        blocker.orchestrate(task("Q2", u.member("m1")))
        blocker.wait_event("attempt.started", task="Q2")
        waiting = t.run("排在另一个工作区后面的目标")
        c(waiting.status() == "queued", "队列跨工作区：另一个工作区的目标占着唯一的位置，这个目标排队")
        ctx.server.stop()
        ctx.server.start()
        blocker.wait_event("run.recovered")
        c(blocker.status() == "running" and blocker.has("run.interrupted")[-1]["data"].get("after") == "goes_on",
          "服务重启后，在做的目标记为中断并自动继续，拿回自己的位置")
        c(waiting.status() == "queued", "排队的目标仍在排队，没有抢走自动继续的目标的位置")
        blocker.act("cancel")
        blocker.wait_status("cancelled")
        waiting.wait_status("running")
        c([e["type"] for e in waiting.events()][:3] == ["run.queued", "run.starting", "run.created"],
          "位置空出后，排队的目标自动开始（排队、开始准备、开始，各一次）")
        ctx.ui(c, waiting.id, lambda r, s: r["status"] == "running" and r["live"], "重启后开始的目标在应用里是进行中")
        waiting.act("cancel")
        waiting.wait_status("cancelled")
