"""Workspace Loops: waking on a rhythm, dispatching runs once per key, asking the person, pausing, deleting."""

import json

from lib import Mcp, drawn, wait
from scenarios import scenario


@scenario("loops")
def loops(ctx, c):
    """工作区值班：新建后醒来一次并按节奏睡下；会话只有值班的工具，凭据不能冒充别的会话；派发按 key 只派一次，派出的任务带来源；提问进收件箱和值班页，在应用里回答后唤醒它，回答写进醒来的提示；暂停、手动唤醒、删除；应用的工作区页、侧栏和值班页（派出的任务和数量）都与服务端一致"""
    t = ctx.team("loops", members=("m1",))
    ws = t.ws["id"]
    made = ctx.api("POST", f"/api/workspaces/{ws}/loops",
                   {"prompt": "看看有什么新 issue", "rhythm": {"kind": "interval", "minutes": 60}, "wakeNow": True})
    loop_id, run = made["id"], made["run"]
    c(made["name"] == "" and made["title"] == "看看有什么新 issue", "新建时不填名字：醒来之前用指令的第一行")

    def events():
        return ctx.api("GET", f"/api/runs/{run}")["events"]

    def kinds(kind):
        return [e for e in events() if e["type"] == kind]

    def view():
        return next(l for l in ctx.api("GET", f"/api/loops?workspace={ws}")["loops"] if l["id"] == loop_id)

    wait(lambda: kinds("loop.slept"), "第一次醒来结束", 60)
    woke = kinds("loop.woke")[0]["data"]
    c(woke["reason"] == "manual" and woke["n"] == 1, "新建并醒来一次：第 1 次醒来，原因是手动")
    sent = next(e for e in events() if e["type"] == "user.message")
    c(sent["actor"] == "janus" and sent["data"]["text"].endswith("看看有什么新 issue"), "醒来的提示来自 Janus，以值班的指令结尾")
    v = view()
    slept = kinds("loop.slept")[0]["data"]
    c(v["status"]["phase"] == "sleeping" and v["status"]["nextWakeAt"] == slept["until"] and v["status"]["wakes"] == 1,
      "值班睡下，下次醒来的时刻就是 loop.slept 记下的")
    c(v["name"] == v["title"] == "标题：看看有什么新 issue", "第一次醒来后，Agent 给会话起的标题就是值班的名字")
    c((v["status"]["lastSummary"] or "").startswith("看过了："), f"这次醒来的摘要是回复的第一行（{v['status']['lastSummary']}）")

    janus = t.fake.board(run, "lead")
    names = sorted(janus.tools())
    c(names == ["ask_user", "dispatch", "dispatches", "next_wake", "save_state"], f"值班的会话只有值班的工具（{names}）")
    # With the URL its session was given: the board's tools are refused, and so is another session's name.
    text, err = janus.call("orchestrate", {"tasks": []})
    c(err and "没有这个工具" in text, f"值班的会话调用看板工具被拒绝（{text[:40]}）")
    status, _ = Mcp(janus.url.rsplit("/", 1)[0] + "/m1").post({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})
    c(status == 403, f"用值班会话的凭据冒充别的会话被拒绝（HTTP {status}）")
    text, err = janus.call("next_wake", {"after_seconds": 120, "reason": "试试"})
    c(err, f"固定间隔的值班不能自己选下次醒来（{text[:40]}）")
    text, err = janus.call("dispatch", {"goal": "修复导出时文件名乱码", "key": "github:o/r#7", "source_url": "https://example.com/7"})
    c(not err, f"派发一个任务：{text[:60]}")
    sent_runs = ctx.api("GET", f"/api/runs?loop={loop_id}")["runs"]
    c(len(sent_runs) == 1 and sent_runs[0]["origin"] == "loop" and sent_runs[0]["dispatchKey"] == "github:o/r#7"
      and sent_runs[0]["sourceUrl"] == "https://example.com/7", "派出的任务记下来自哪个值班、key 和来源链接")
    dispatched = sent_runs[0]["id"]
    text, err = janus.call("dispatch", {"goal": "再修一次", "key": "github:o/r#7"})
    c(not err and dispatched in text and len(ctx.api("GET", f"/api/runs?loop={loop_id}")["runs"]) == 1, "同一个 key 不再派发，告诉值班已有的任务")
    if ctx.app:
        def row(s):
            return next((r for r in (s.get("histories") or {}).get(ws, []) if r["id"] == dispatched), None)
        ctx.app.until(lambda s: (row(s) or {}).get("origin") == "loop", "工作区的任务记录标出来自值班", "reload", f"open home:{ws}")
        ctx.app.until(lambda s: any(l["id"] == loop_id and l["wakes"] == 1 for l in s.get("loops", [])), "工作区页列出值班", f"open home:{ws}")
        ctx.app.do("shot loops-home")
        c(True, "应用：工作区页的值班一节和来自值班的任务记录（loops-home.png）")
        ctx.app.do(f"loop-new {ws}", "shot loop-editor", "close-sheet")
        c(drawn(ctx.app.picture("loop-editor")), "应用：新建值班的表单画出来了（loop-editor.png）")
        # Its page lists what it dispatched as the server has it, with the count in its header.
        listed = [{"id": r["id"], "key": r["dispatchKey"], "status": r["status"]} for r in ctx.api("GET", f"/api/runs?loop={loop_id}")["runs"]]
        page = ctx.app.until(lambda s: ((s.get("run") or {}).get("loop") or {}).get("dispatched") == listed
                             and s["run"]["loop"]["dispatchedCount"] == len(listed), "值班页列出派出的任务", f"open run:{run}")
        c(page["run"]["loop"]["title"] == v["title"], "应用：值班页的派出任务、数量和名字与服务端一致")

    # A question goes to the inbox; the answer wakes the Loop.
    text, err = janus.call("ask_user", {"question": "先修哪个？", "why": "只有你知道优先级", "options": ["导出", "设置"]})
    c(not err, "值班用 ask_user 问用户，不等回答")
    asked = wait(lambda: kinds("question.escalated"), "问题记在值班的会话里", 30)[0]
    if ctx.app:
        s = ctx.app.until(lambda s: any(e["run"] == run and e["kind"] == "question" for e in s["open"]), "问题进了收件箱", "open inbox")
        c(True, "应用：值班的问题在收件箱里")
        ctx.app.until(lambda s: any(i["key"] == asked["data"]["key"] for i in (s.get("run") or {}).get("inbox", [])),
                      "值班页也能回答这个问题", f"open run:{run}")
        c(True, "应用：值班页列出待回答的问题")
        # Answer with the inbox's first quick answer, as the person would.
        i = next(n for n, e in enumerate(s["open"]) if e["run"] == run and e["kind"] == "question")
        ctx.app.do("open inbox", f"inbox-act {i}:0")
    else:
        ctx.api("POST", f"/api/runs/{run}/actions", {"type": "answer", "key": asked["data"]["key"], "answer": "导出"})
    wait(lambda: len(kinds("loop.woke")) == 2, "回答唤醒值班", 60)
    c(kinds("loop.woke")[1]["data"]["reason"] == "answer", "回答之后值班以 answer 醒来")
    wait(lambda: len(kinds("loop.slept")) == 2, "第二次醒来结束", 60)
    prompts = [json.loads(l[len("loop prompt "):]) for l in t.fake.calls().splitlines() if l.startswith("loop prompt ")]
    c("Q: 先修哪个？\n  A: 导出" in prompts[-1], "值班醒来的提示里带着问题和回答")

    # Paused: no clock, still wakes by hand.
    paused = ctx.api("POST", f"/api/loops/{loop_id}/pause", {})
    c(paused["state"] == "paused" and paused["status"]["phase"] == "paused" and paused["status"]["nextWakeAt"] is None, "暂停后没有下次醒来的时刻")
    if ctx.app:
        ctx.ui(c, run, lambda r, s: any(l["id"] == loop_id and l["phase"] == "paused" for l in s.get("loops", [])),
               "值班页显示已暂停", pages=("board",))
    ctx.api("POST", f"/api/loops/{loop_id}/wake", {})
    wait(lambda: len(kinds("loop.woke")) == 3, "暂停时手动唤醒", 60)
    c(kinds("loop.woke")[2]["data"]["reason"] == "manual", "暂停的值班仍可以手动唤醒")
    wait(lambda: len(kinds("loop.slept")) == 3, "手动醒来结束", 60)
    resumed = ctx.api("POST", f"/api/loops/{loop_id}/resume", {})
    c(resumed["state"] == "active" and resumed["status"]["nextWakeAt"] is not None, "继续后按节奏排下次醒来")

    # Deleting it removes its session; the run it dispatched stays.
    ctx.api("DELETE", f"/api/loops/{loop_id}")
    c(not ctx.api("GET", f"/api/loops?workspace={ws}")["loops"], "删除值班")
    code, _ = ctx.api_error("GET", f"/api/runs/{run}")
    c(code == 404, "值班的会话一起删除")
    c(ctx.api("GET", f"/api/runs/{dispatched}")["run"]["origin"] == "loop", "它派出的任务保留")
    ctx.api("POST", f"/api/runs/{dispatched}/actions", {"type": "cancel"})
