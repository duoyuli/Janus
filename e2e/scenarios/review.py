"""Reviewing a delivery (docs/architecture/20261031-frontend.md#review): the lead's conclusion and
the questions it leaves, one round at a time, and no stopping once a result is out."""

from lib import wait
from scenarios import scenario, task

REPORT = "## 结论\n笔记写好了。\n\n## 需要你决定\n- 要不要发布？\n\n## 改了什么\nnotes.md 写了一份笔记"


@scenario("review")
def review(ctx, c):
    """交付的审阅：负责人的结论和要你决定的事、第二次交付只审这一轮、交付后不能停止；应用和服务端逐项一致"""
    t = ctx.team("review", members=("m1",), script={"T1": ["write:notes.md"]})
    run = t.run("写一份笔记")
    run.orchestrate(task("T1", t.member("m1")))
    run.read_board(lambda b: "[完成] T1" in b)
    first = run.finish(REPORT)["data"]
    c(first["headline"] == "笔记写好了。" and first["decisions"] == ["要不要发布？"],
      f"交付按标题切出结论和要你决定的事（{first['headline']!r}，{first['decisions']}）")
    c("## 结论" not in first["summary"] and "notes.md 写了一份笔记" in first["summary"], "报告的其余部分整篇保留")
    c(first["merge"]["state"] == "merged" and first["changed"] == ["notes.md"],
      f"合并结果和这一轮改的文件（{first['merge']['state']}，{first['changed']}）")
    c([a["task"] for a in first["acceptance"]] == ["T1"] and first["risks"] == [],
      "交付的风险只放 Janus 查出来的，任务自己的风险留在它的记录里")
    st = ctx.ui(c, run.id, lambda r, s: (r.get("review") or {}).get("stage") == "awaiting"
                and r["review"]["headline"] == first["headline"] and r["review"]["decisions"] == first["decisions"]
                and r["review"]["changed"] == first["changed"] and r["review"]["merge"] == "merged" and r["review"]["tasks"] == ["T1"]
                and (r["review"]["signals"] or [{}])[0] == {"source": "lead", "text": "要不要发布？"},
                "交付卡片的结论、要你决定的事、改的文件和子任务与服务端一致，要你决定的事排在最前", pages=("page board",))
    if st:
        view = ctx.api("GET", f"/api/runs/{run.id}/view")
        served = [{"id": t["id"], "bars": len(t["bars"]), "waiting": t.get("waiting"), "result": (t.get("result") or {}).get("title")}
                  for t in view["timeline"]["tasks"]]
        marks = [f"{m['kind']}:{m['title']}" for m in view["timeline"]["milestones"]]
        c(st["run"]["timeline"] == {"tasks": served, "milestones": marks} and served[0]["result"],
          f"应用：进展时间线的子任务、尝试、结论和上面一栏的决定与服务端一致（{st['run']['timeline']}）")
        rows = [{"task": t["task"], "metrics": t["metrics"], "markers": t["markers"], "open": t["needsAttention"],
                 "strength": t["strength"]["text"]} for t in view["delivery"]["tasks"]]
        rv = st["run"]["review"]
        c(rv["facts"] == view["delivery"]["facts"] and rv["rows"] == rows and rows[0]["metrics"],
          f"应用：交付的一行数字和每个子任务的一行（通过数、方法计数、标记、是否展开、证据强度）与服务端一致（{rv['facts']}，{rv['rows']}）")

    code, text = run.act_error("cancel")
    c(code == 400 and "已经交付过" in text, f"交付后停止被拒绝：结果已经在分支上（{code}）")

    # The person asks for more: the lead writes a file, and the run is delivered again, this round alone.
    run.act("message", text="再补一份说明 [write extra.md]", agent="lead")
    wait(lambda: len(run.has("run.delivered")) >= 2, "第二次交付")
    second = run.has("run.delivered")[1]["data"]
    c(second["changed"] == ["extra.md"] and second["leadEdits"] == ["extra.md"] and second["acceptance"] == [],
      f"第二次交付只含这一轮：负责人自己改的文件，没有上一轮的子任务（{second['changed']}，{second['acceptance']}）")
    ctx.ui(c, run.id, lambda r, s: (r.get("review") or {}).get("n") == 2 and r["review"]["stage"] == "awaiting"
           and r["review"]["requests"] == ["再补一份说明 [write extra.md]"] and r["review"]["changed"] == second["changed"]
           and r["review"]["tasks"] == [] and [x["source"] for x in r["review"]["signals"]] == ["janus"],
           "第二次交付：引用你的追加要求，只列这一轮改的文件，负责人自己改的文件列在要看的事里", pages=("board",))
    if ctx.app:
        key = f"{run.id}/delivered-2"
        st = ctx.app.until(lambda s: any(e["key"] == key for e in s.get("open", [])), "收件箱出现第二次交付", "reload", "open inbox")
        mine = [e["key"] for e in st["open"] if e["run"] == run.id]
        c(mine == [key], f"应用：收件箱里这个 Run 只有第二次交付一条（{mine}）")
