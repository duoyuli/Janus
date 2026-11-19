"""What a session shows, and how parallel collaborators share one working directory."""

from scenarios import scenario, task


@scenario("sessions")
def sessions(ctx, c):
    """会话视图与协作：思考、计划、命令、工具 diff、模式和用量；@ 文件引用；同时改同一文件的冲突；认领、拆分、痕迹对账"""
    t = ctx.team("sessions", members=("m1", "m2", "m3", "m4", "m5"), files={"README.md": "e2e\n", "pay.js": "a\n"}, script={
        "T1": ["rich"], "T2": ["edit:shared.txt"], "T3": ["edit:shared.txt"], "T4": ["slow"], "T5": ["write:b.txt"]})
    m = t.member
    run = t.run("看得见的会话")
    lead = run.lead()
    run.orchestrate(task("T1", m("m1"), owns=["pay.js"]), task("T2", m("m2")), task("T3", m("m3")),
                    task("T4", m("m4"), owns=["docs"]), task("T5", m("m5"), owns=["a.txt"]))

    # A rich session: everything the agent shows becomes events.
    run.wait_event("task.succeeded", task="T1")
    kinds = {e["type"] for e in run.events() if e.get("task") == "T1"}
    for kind, what in (("agent.thought", "思考"), ("agent.plan", "计划"), ("session.commands", "斜杠命令"), ("session.usage", "用量"), ("session.controls", "控制项")):
        c(kind in kinds, f"会话中能看到 Agent 的{what}（{kind}）")
    tool = [e for e in run.events() if e["type"] == "tool.call" and e.get("task") == "T1" and e["data"].get("diffs")]
    c(tool, "进行中的工具调用完成后带着 diff")
    c(run.has("session.control", task="T1", data__id="mode"), "Agent 自己切换模式也记录下来")

    # Two tasks that may run together edit the same file.
    run.wait_event("task.conflict", timeout=60)
    c(True, "两个同时执行的任务改了同一个文件，报告给负责人")

    # A collaborator at work: claims, peers, a split request.
    t4 = t.fake.board(run.id, "T4")
    text, err = t4.call("claim", {"paths": ["a.txt"]})
    c(err, f"认领别的任务负责的文件被拒绝（{text[:50]}）")
    text, err = t4.call("claim", {"paths": ["docs/guide.md", "NOTES.md"]})
    c(not err and run.has("task.claimed", task="T4"), "认领没有归属的文件")
    text, err = t4.call("peers", {})
    c(not err and "T5" in text, "peers 列出其他任务")
    text, err = t4.call("request_split", {"reason": "太大", "completed": "读完了代码", "subtasks": [
        {"title": "写文档", "owns": ["docs"], "accept": ["文档写完"]}, {"title": "写示例", "owns": ["NOTES.md"], "accept": ["示例能跑"]}]})
    c(not err, "协作者提出拆分任务")
    board = run.read_board(lambda b: "[请求拆分] T4" in b and "[冲突]" in b and "[完成] T5" in b)
    c("痕迹核对" in board.split("[完成] T5", 1)[1][:2000], "T5 改了负责范围以外的文件，完成消息带着痕迹核对")
    gaps = run.has("task.succeeded", task="T5")[-1]["data"].get("traceGaps")
    c(gaps, f"task.succeeded 记下 traceGaps：{gaps}")
    lead.call("cancel", {"tasks": ["T4"], "reason": "按拆分重新安排"})
    run.wait_event("task.skipped_by_decision", task="T4")

    run.finish("会话都看过了")

    # `@path` references a workspace file; the run's file search finds it.
    files = ctx.api("GET", f"/api/runs/{run.id}/files?q=READ")["files"]
    c(files and files[0] == "README.md", f"输入框的 @ 菜单能搜到工作区文件：{files}")
    run.act("message", agent="lead", text="看看 @README.md 和 @missing.md")
    t.fake.wait_call("links README.md")
    c("missing.md" not in t.fake.calls().split("links README.md", 1)[1].split("\n", 1)[0], "@ 引用只带工作区里存在的文件")
    ctx.ui(c, run.id, lambda r, s: next(x for x in r["tasks"] if x["id"] == "T1")["status"] == "succeeded", "T1 的会话已完成",
           pages=("task:T2", "task:T1", "density detail", "draft T1 /", "agent:lead"))
    if ctx.app:
        ctx.app.do("density normal")
