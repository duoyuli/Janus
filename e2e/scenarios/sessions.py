"""What a session shows, and how parallel collaborators share one working directory."""

from scenarios import scenario, task


@scenario("sessions")
def sessions(ctx, c):
    """会话视图与协作：思考、计划、命令、工具 diff、模式和用量；@ 文件引用；同时改同一文件的冲突；拆分提议、痕迹对账"""
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

    # A collaborator at work: the board, a question with a split proposal.
    t4 = t.fake.board(run.id, "T4")
    text, err = t4.call("board", {})
    c(not err and "T5" in text, "janus.board 列出其他任务")
    text, err = t4.call("ask", {"text": "按这样拆可以吗", "split": {"reason": "太大", "completed": "读完了代码", "subtasks": [
        {"title": "写文档", "owns": ["docs"], "accept": ["文档写完"]}, {"title": "写示例", "owns": ["NOTES.md"], "accept": ["示例能跑"]}]}})
    c(not err and run.has("task.split_requested", task="T4"), "协作者提问时附上拆分提议，负责人立即收到")
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
           pages=("task:T2", "task:T1", "density summary", "task:T1", "density detail", "draft task:T1 /", "agent:lead"))
    if ctx.app:
        conversation_in_turns(ctx, c, run)


def conversation_in_turns(ctx, c, run):
    """The app lays T1's session out in turns, from the same events the server holds
    (docs/architecture/20261031-frontend.md#session_turns)."""
    evs = [e for e in run.events() if e.get("task") == "T1"]
    commands = [x["name"] for x in next(e for e in reversed(evs) if e["type"] == "session.commands")["data"]["commands"]]
    hint = next(x["hint"] for x in next(e for e in reversed(evs) if e["type"] == "session.commands")["data"]["commands"] if x["name"] == "review")
    edited = {d["path"] for e in evs if e["type"] == "tool.call" for d in e["data"].get("diffs") or []}

    def conv(st):
        return st["run"]["conversations"]["task:T1"]

    # The server lays the rows out at the level chosen and offers the commands for the draft: both
    # come back a moment after the commands.
    def summary(s):
        rows = conv(s)["rows"]
        head = next((r for r in rows if r["kind"] == "head"), None)
        return conv(s)["suggestions"] and head and head["open"] is ("reply" not in [r["kind"] for r in rows])

    st = ctx.app.until(summary, "摘要档的会话和 / 的建议", f"open run:{run.id}", "density summary", "draft task:T1 /")
    rows = conv(st)["rows"]
    kinds = [r["kind"] for r in rows]
    c(kinds[0] == "section" and "system" in rows[0]["injected"],
      f"每次尝试开头列出 Janus 注入的系统层，留在对话里：{rows[0]}")
    head = next((r for r in rows if r["kind"] == "head"), None)
    c(head and head["agent"] and f"改 {len(edited)} 个文件" in head["stats"] and not head["running"], f"Agent 的一行写出这一轮做了什么：{head}")
    # A turn ending in a reply folds to it; one that only handed in its result keeps its work in view.
    replied = "reply" in kinds
    c(head["open"] is (not replied) and ("activity" in kinds) is (not replied), f"摘要档：有回复的一轮收起，没有回复的保持展开：{kinds}")
    c("item" in kinds and any(r.get("type") == "result.submitted" for r in rows), "提交的结果始终在对话里")
    files = next(r for r in rows if r["kind"] == "edits")["files"]
    c({f["path"] for f in files} == edited and files[0]["added"] == 1, f"「改了 N 个文件」和服务端的 diff 一致：{files}")
    labels = [x["label"] for x in conv(st)["suggestions"]]
    c(labels == ["/" + n for n in commands], f"输入 / 列出 Agent 的斜杠命令：{labels}")

    st = ctx.app.until(lambda s: conv(s)["ghost"] and any(r["kind"] == "head" and r["open"] for r in conv(s)["rows"]),
                       "详细档的会话和选中命令的参数", "density detail", "draft task:T1 /review ")
    kinds = [r["kind"] for r in conv(st)["rows"]]
    c("activity" in kinds and next(r for r in conv(st)["rows"] if r["kind"] == "head")["open"] is True, f"详细档：每一轮都展开：{kinds}")
    c(conv(st)["ghost"] == hint, f"选中命令后灰字显示它的参数：{conv(st)['ghost']}")
    c(conv(st)["preview"] == {"text": "", "tools": 0}, "会话结束后没有残留的实时预览")
    ctx.app.do("draft task:T1 ")
