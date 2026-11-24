"""A workspace's team memory: notes kept through the janus tools, read by later runs, kept to their workspace, consolidated."""

from lib import drawn, wait
from scenarios import scenario, task


@scenario("memory")
def memory(ctx, c):
    """团队记忆：负责人和协作者用 remember 记笔记，记下是谁在哪个任务里写的；recall 按 id 读、按词搜；不收空笔记、未知类型和像凭据的内容；别的工作区的笔记读不到也改不了；只有整理能删笔记；协作者的会话和之后任务的负责人只拿到用法，需要时按关键词检索笔记；你改过的笔记算你的；现在整理开一个负责人的整理会话，只有记忆工具，同一时间只有一个；整理一轮后记下改了什么、已交付，收件箱里出现「团队记忆整理好了」；应用的工作区页（笔记、来源、整理）、笔记的表单和整理页都与服务端一致"""
    t = ctx.team("memory", members=("m1",), script={"T1": ["hold:t1-go"]})
    other = ctx.team("memory-b", members=("m1",))
    ws = t.ws["id"]

    def notes(w=ws):
        return ctx.api("GET", f"/api/workspaces/{w}/memories")["memories"]

    # Another workspace's note, written by its own lead.
    rb = other.run("别的工作区的目标")
    text, err = rb.lead().call("remember", {"kind": "fact", "title": "别的工作区的事", "hook": "只属于 memory-b", "body": "b"})
    c(not err, f"另一个工作区的负责人记下一条笔记（{text[:30]}）")
    foreign = notes(other.ws["id"])[0]["id"]

    r = t.run("整理测试命令")
    lead = r.lead()
    names = lead.tools()
    c("recall" in names and "remember" in names and "forget" not in names, f"负责人能读写团队记忆，不能删（{names}）")
    text, err = lead.call("remember", {"kind": "howto", "title": "怎么跑测试", "hook": "先装依赖再跑测试", "body": "运行 just setup，再运行 just test。"})
    c(not err, f"负责人记下一条笔记（{text[:30]}）")
    r.orchestrate(task("T1", t.member("m1")))
    t1 = t.fake.board(r.id, "T1")
    text, err = t1.call("remember", {"kind": "pitfall", "title": "端口被占用", "hook": "别用 7878", "body": "测试一律用空闲端口。"})
    c(not err, f"协作者记下一条笔记（{text[:30]}）")
    saved = notes()
    by_title = {m["title"]: m for m in saved}
    howto, pitfall = by_title.get("怎么跑测试"), by_title.get("端口被占用")
    c(len(saved) == 2 and howto and pitfall and howto["who"] == "lead" and howto["run"] == r.id and pitfall["who"] == t.member("m1")
      and pitfall["author"] == "memory m1" and pitfall["runGoal"] == "整理测试命令",
      "每条笔记记着是谁在哪个任务里写的")
    c(len(r.has("memory.saved")) == 2, "写笔记记在任务的事件里")

    text, err = t1.call("recall", {"id": howto["id"]})
    c(not err and "just setup" in text, "按 id 读到整条笔记")
    text, _ = lead.call("recall", {"query": "空闲 端口"})
    c("端口被占用" in text and "怎么跑测试" not in text, "按词搜到对的笔记")
    for args, why in (({"kind": "fact", "title": "", "hook": "h", "body": "b"}, "空标题"),
                      ({"kind": "rumor", "title": "t", "hook": "h", "body": "b"}, "未知类型"),
                      ({"kind": "fact", "title": "令牌", "hook": "h", "body": "token ghp_abcdefghijklmnopqrstuvwxyz0123456789"}, "像凭据的内容")):
        text, err = lead.call("remember", args)
        c(err, f"{why}的笔记被拒绝（{text[:30]}）")
    text, err = lead.call("recall", {"id": foreign})
    c(err and "只属于" not in text, "读不到别的工作区的笔记")
    text, err = t1.call("remember", {"id": foreign, "kind": "fact", "title": "改写", "hook": "h", "body": "b"})
    c(err and notes(other.ws["id"])[0]["title"] == "别的工作区的事", "改不了别的工作区的笔记")
    for who, mcp in (("负责人", lead), ("协作者", t1)):
        text, err = mcp.call("forget", {"id": howto["id"], "reason": "x"})
        c(err and "没有这个工具" in text, f"{who}不能删笔记")
    c(len(notes()) == 2, "笔记都还在")

    layer = next((e["data"]["prompt"] for e in r.events() if e["type"] == "session.prompt" and e.get("task") == "T1"
                  and (e.get("data") or {}).get("channel") == "system"), "")
    c("Team memory:" in layer and "janus.recall" in layer and howto["id"] not in layer, "协作者的会话拿到团队记忆的用法，笔记本身不在里面")
    t.fake.touch("t1-go")
    r.finish()

    # The next run's lead is told how many notes there are and searches them; the notes are not
    # in its instructions.
    r2 = t.run("加一个命令")
    lead2 = r2.lead()
    first = next((e["data"]["prompt"] for e in r2.events() if e["type"] == "session.prompt" and not e.get("task")
                  and (e.get("data") or {}).get("channel") == "system"), "")
    c("has 2 notes" in first and howto["id"] not in first and "端口被占用" not in first, "之后任务的负责人只知道有几条笔记，笔记不在说明里")
    text, _ = lead2.call("recall", {"query": "测试的端口被占用"})
    c("端口被占用" in text and foreign not in text, "之后任务的负责人按需检索到这个工作区的笔记，别的工作区的检索不到")
    r2.finish()

    # The person's change makes the note theirs.
    edited = ctx.api("PUT", f"/api/memories/{pitfall['id']}", {"hook": "测试别用 7878"})
    c(edited["who"] == "user" and edited["author"] == "你" and edited["hook"] == "测试别用 7878", "你改过的笔记算你写的")
    code, _ = ctx.api_error("PUT", "/api/memories/m-missing", {"hook": "x"})
    c(code == 404, f"改不存在的笔记返回 404（{code}）")

    if ctx.app:
        def shown(s):
            return ((s.get("memories") or {}).get(ws) or {}).get("notes") or []
        server = notes()
        st = ctx.app.until(lambda s: [n["id"] for n in shown(s)] == [m["id"] for m in server], "工作区页列出团队记忆", "reload", f"open home:{ws}")
        rows = {n["id"]: n for n in shown(st)}
        c(all(rows[m["id"]]["title"] == m["title"] and rows[m["id"]]["hook"] == m["hook"] for m in server)
          and rows[pitfall["id"]]["kind"] in ("坑", "Pitfall") and rows[pitfall["id"]]["source"].startswith(("你 · ", "You · "))
          and "memory 负责人" in rows[howto["id"]]["source"],
          "应用：工作区页的笔记（类型、标题、摘要、来源）与服务端一致")
        st = ctx.app.do(f"open home:{ws}", "shot memory-home")
        c(st.get("screen") == f'Optional(Janus.Screen.home("{ws}"))'
          and [n["id"] for n in shown(st)] == [m["id"] for m in server]
          and (st.get("home") or {}).get("memory") in (f"{len(server)} 条笔记", f"{len(server)} notes")
          and drawn(ctx.app.picture("memory-home")),
          f"应用：工作区首页名称旁的团队记忆写着 {len(server)} 条笔记，笔记与服务端一致（memory-home.png）")
        # One batch: scenarios running beside this one move the app between batches.
        st = ctx.app.do(f"open home:{ws}", f"memory {howto['id']}", "shot memory-editor")
        c(st["editingMemory"] == howto["id"] and drawn(ctx.app.picture("memory-editor")), "应用：笔记的表单打开了这条笔记（memory-editor.png）")
        ctx.app.do("close-sheet")

    # Consolidating: the lead's own session over the notes, one at a time.
    if ctx.app:
        ctx.app.do(f"consolidate {ws}")
        dream = wait(lambda: ctx.api("GET", f"/api/workspaces/{ws}/memories")["consolidation"]["running"], "整理开始", 30)
    else:
        dream = ctx.api("POST", f"/api/workspaces/{ws}/memories/consolidate")["run"]
    code, why = ctx.api_error("POST", f"/api/workspaces/{ws}/memories/consolidate")
    c(code == 409 and "正在整理" in str(why), f"同一时间只有一次整理（{code}）")
    row = ctx.api("GET", f"/api/runs/{dream}")["run"]
    c(row["kind"] == "dream" and row["config"]["workspace"]["id"] == ws, "整理是这个工作区的一个整理会话")
    keeper = t.fake.board(dream, "lead")
    names = sorted(keeper.tools())
    c(names == ["forget", "recall", "remember"], f"整理会话只有记忆工具（{names}）")
    text, err = keeper.call("forget", {"id": pitfall["id"], "reason": "已写进测试脚本"})
    c(not err and len(notes()) == 1, f"整理删掉一条笔记（{text[:30]}）")
    t.fake.wait_call("dream prompt")
    t.fake.touch("dream-go", "删了 1 条：端口的事已写进测试脚本")
    done = wait(lambda: [e for e in ctx.api("GET", f"/api/runs/{dream}")["events"] if e["type"] == "memory.consolidated"], "整理结束", 60)[0]["data"]
    c(done["forgotten"] == 1 and done["saved"] == 0 and done["summary"] == "删了 1 条：端口的事已写进测试脚本", "整理记下改了什么")
    wait(lambda: ctx.api("GET", f"/api/runs/{dream}")["run"]["status"] == "delivered", "整理已交付", 30)
    view = ctx.api("GET", f"/api/workspaces/{ws}/memories")["consolidation"]
    c(view["running"] is None and view["last"]["run"] == dream, "工作区记下上一次整理")

    if ctx.app:
        st = ctx.app.until(lambda s: any(e["run"] == dream and e["kind"] == "dream" for e in s.get("open", [])),
                           "收件箱出现整理好了", "reload", "open inbox")
        entry = next(e for e in st["open"] if e["run"] == dream)
        c(entry["canWait"] and entry["title"] in ("团队记忆整理好了", "Team memory consolidated"), "应用：收件箱里是「团队记忆整理好了」，可以推迟，看过可以忽略")
        st = ctx.ui(c, dream, lambda run, s: run["dream"] and (run.get("consolidated") or {}).get("forgotten") == 1, "整理页显示整理的结果")
        ctx.app.do(f"open run:{dream}", "shot memory-dream")
        c(drawn(ctx.app.picture("memory-dream")), "应用：整理页画出来了（memory-dream.png）")
        st = ctx.app.until(lambda s: ((s.get("memories") or {}).get(ws) or {}).get("last") == dream
                           and [n["id"] for n in ((s.get("memories") or {}).get(ws) or {}).get("notes", [])] == [howto["id"]],
                           "工作区页跟上整理的结果", f"open home:{ws}")
        c(True, "应用：工作区页的笔记和上次整理跟上了服务端")
