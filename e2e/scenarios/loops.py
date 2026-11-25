"""Workspace Loops: waking on a schedule, dispatching runs once per key, asking the person, pausing, deleting."""

import json

from lib import Failure, Mcp, drawn, wait
from scenarios import scenario


@scenario("loops")
def loops(ctx, c):
    """工作区值班：时间表由服务端检查并算出接下来的时刻；新建后醒来一次并按时间表睡到下一个整点，第一次开工自己起名，开工的消息只有值班的提示；会话只有值班的工具，凭据不能冒充别的会话；派发按 key 只派一次，派出的任务带来源；值班只能给自己派出、进行中的任务转话；派出的任务结束时以 dispatch 唤醒值班，结果写进醒来的提示；提问进收件箱和值班页，在应用里回答后唤醒它，回答写进醒来的提示；暂停、手动唤醒、删除；应用的工作区页（待处理里的值班、不重复的任务记录）、侧栏的数字和值班页（派出的任务和数量、会话里每次开工和收工的时刻）都与服务端一致"""
    t = ctx.team("loops", members=("m1",))
    ws = t.ws["id"]
    preview = ctx.api("GET", "/api/loops/schedule?cron=0+9+*+*+1-5&tz=Asia/Shanghai")
    c(len(preview["next"]) == 3 and preview["tz"] == "Asia/Shanghai", f"时间表由服务端算出接下来三次（{preview['next']}）")
    code, why = ctx.api_error("GET", "/api/loops/schedule?cron=0+9+*+*")
    c(code == 400 and "5 段" in str(why), f"不是 5 段的 cron 被拒绝，说明原因（{code}）")
    made = ctx.api("POST", f"/api/workspaces/{ws}/loops",
                   {"prompt": "看看有什么新 issue", "schedule": {"cron": "0 * * * *", "tz": "UTC"}, "wakeNow": True})
    loop_id, run = made["id"], made["run"]
    c(made["name"] == "" and made["title"] == "看看有什么新 issue", "新建时不填名字：醒来之前用指令的第一行")

    def events():
        return ctx.api("GET", f"/api/runs/{run}")["events"]

    def kinds(kind):
        return [e for e in events() if e["type"] == kind]

    def view():
        return next(l for l in ctx.api("GET", f"/api/loops?workspace={ws}")["loops"] if l["id"] == loop_id)

    def last_seq():
        ev = events()
        return ev[-1]["seq"] if ev else 0

    def wake_for(reason, after, what):
        """The wake for `reason` after event `after`, once the sleep after it is recorded. Wakes are
        picked by reason, not counted: the Loop sleeps to the next full hour, and a test that
        crosses the hour gets a timer wake in between."""
        def found():
            return next((e for e in kinds("loop.woke") if e["seq"] > after and e["data"].get("reason") == reason), None)
        try:
            w = wait(found, what, 60)
        except Failure as e:
            seen = [(x["data"].get("reason"), x["seq"]) for x in kinds("loop.woke")]
            raise Failure(f"{e}：{after} 之后没有 {reason} 的开工，开工记录 {seen}") from None
        wait(lambda: any(x["seq"] > w["seq"] for x in kinds("loop.slept")), f"{what}之后睡下", 60)
        return w

    def prompts():
        return [json.loads(l[len("loop prompt "):]) for l in t.fake.calls().splitlines() if l.startswith("loop prompt ")]

    wait(lambda: kinds("loop.slept"), "第一次醒来结束", 60)
    woke = kinds("loop.woke")[0]["data"]
    c(woke["reason"] == "manual" and woke["n"] == 1, "新建并马上开工：第 1 次开工，原因是手动")
    sent = next(e for e in events() if e["type"] == "user.message")
    c(sent["actor"] == "janus" and sent["data"]["text"] == "看看有什么新 issue", "开工的消息来自 Janus，只有值班的提示，不带环境信息")
    v = view()
    slept = kinds("loop.slept")[0]["data"]
    c(v["status"]["phase"] == "sleeping" and v["status"]["nextWakeAt"] == slept["until"] and v["status"]["wakes"] == 1
      and slept["until"] % 3_600_000 == 0 and v["status"]["nextWakeReason"] == "timer",
      "值班睡到时间表的下一个整点，下次醒来的时刻就是 loop.slept 记下的")
    layer = next(e for e in events() if e["type"] == "session.prompt" and (e.get("data") or {}).get("channel") == "system")["data"]["prompt"]
    c(v["name"] == "" and "janus.set_name" in layer, "会话标题不当名字；还没有名字时，值班的工作方式里请它自己起名")
    c((v["status"]["lastSummary"] or "").startswith("看过了："), f"这次醒来的摘要是最后一条消息的第一行（{v['status']['lastSummary']}）")

    janus = t.fake.board(run, "lead")
    names = sorted(janus.tools())
    c(names == ["ask_user", "dispatch", "dispatches", "next_wake", "note_dispatch", "recall", "remember", "save_state", "set_name"],
      f"值班的会话只有值班的工具和团队记忆的读写（{names}）")
    text, err = janus.call("set_name", {"name": "issue 分拣"})
    v = view()
    c(not err and v["name"] == v["title"] == "issue 分拣", f"值班用 set_name 给自己起名（{v['name']}）")
    # With the URL its session was given: the board's tools are refused, and so is another session's name.
    text, err = janus.call("orchestrate", {"tasks": []})
    c(err and "没有这个工具" in text, f"值班的会话调用看板工具被拒绝（{text[:40]}）")
    status, _ = Mcp(janus.url.rsplit("/", 1)[0] + "/m1").post({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})
    c(status == 403, f"用值班会话的凭据冒充别的会话被拒绝（HTTP {status}）")
    text, err = janus.call("next_wake", {"after_seconds": 120, "reason": "试试"})
    c(err and "不是一次醒来" in text, f"醒来之外不能约下次醒来（{text[:40]}）")
    text, err = janus.call("dispatch", {"goal": "修复导出时文件名乱码", "key": "github:o/r#7", "source_url": "https://example.com/7"})
    c(not err, f"派发一个任务：{text[:60]}")
    sent_runs = ctx.api("GET", f"/api/runs?loop={loop_id}")["runs"]
    c(len(sent_runs) == 1 and sent_runs[0]["origin"] == "loop" and sent_runs[0]["dispatchKey"] == "github:o/r#7"
      and sent_runs[0]["sourceUrl"] == "https://example.com/7", "派出的任务记下来自哪个值班、key 和来源链接")
    dispatched = sent_runs[0]["id"]
    text, err = janus.call("dispatch", {"goal": "再修一次", "key": "github:o/r#7"})
    c(not err and dispatched in text and len(ctx.api("GET", f"/api/runs?loop={loop_id}")["runs"]) == 1, "同一个 key 不再派发，告诉值班已有的任务")
    # While the run works, what the Loop knows goes to its lead; only to its own runs.
    wait(lambda: ctx.api("GET", f"/api/runs/{dispatched}")["run"]["status"] == "running", "派出的任务开始干活", 60)
    text, err = janus.call("note_dispatch", {"run": dispatched, "text": "文件名里可能有全角空格"})
    told = [e for e in ctx.api("GET", f"/api/runs/{dispatched}")["events"] if e["type"] == "user.message" and e["actor"] == "janus"]
    c(not err and told and "来自工作区的值班" in told[-1]["data"]["text"] and "全角空格" in told[-1]["data"]["text"],
      f"值班把补充转给进行中的派出任务的负责人，写明来自值班（{text[:40]}）")
    text, err = janus.call("note_dispatch", {"run": run, "text": "x"})
    c(err and "不是这个值班派出的" in text, f"不是它派出的任务收不到值班的话（{text[:40]}）")
    if ctx.app:
        def row(s):
            return next((r for r in (s.get("histories") or {}).get(ws, []) if r["id"] == dispatched), None)
        ctx.app.until(lambda s: (row(s) or {}).get("origin") == "loop", "工作区的任务记录标出来自值班", "reload", f"open home:{ws}")
        st = ctx.app.until(lambda s: any(l["id"] == loop_id and l["wakes"] == view()["status"]["wakes"] and l["schedule"] in ("每小时", "Every hour")
                                         for l in s.get("loops", [])), "工作区页列出值班和它的时间表", f"open home:{ws}")
        c(loop_id in ((st.get("home") or {}).get("loops") or []), "应用：值班列在工作区页右边的值班一栏")
        ctx.app.do("shot loops-home")
        c(True, "应用：工作区页的值班一节和来自值班的任务记录（loops-home.png）")
        ctx.app.do(f"loop-new {ws}", "shot loop-editor", "close-sheet")
        c(drawn(ctx.app.picture("loop-editor")), "应用：新建值班的表单画出来了（loop-editor.png）")
        # Its page lists what it dispatched as the server has it, with the count in its header.
        listed = [{"id": r["id"], "key": r["dispatchKey"], "status": r["status"]} for r in ctx.api("GET", f"/api/runs?loop={loop_id}")["runs"]]
        page = ctx.app.until(lambda s: ((s.get("run") or {}).get("loop") or {}).get("dispatched") == listed
                             and s["run"]["loop"]["dispatchedCount"] == len(listed), "值班页列出派出的任务", f"open run:{run}")
        c(page["run"]["loop"]["title"] == v["title"], "应用：值班页的派出任务、数量和名字与服务端一致")
        # Each wake is marked in its session where the server recorded it: which one, when and why it
        # started; when it ended, how long it took and when the next one is.
        def marked(s):
            server = [e for e in events() if e["type"] in ("loop.woke", "loop.slept")]
            got = ((s.get("run") or {}).get("loop") or {}).get("marks") or []
            return [(m["seq"], m["type"]) for m in got] == [(e["seq"], e["type"]) for e in server] and (server, got)
        page = ctx.app.until(marked, "值班页的会话标出每次开工和收工", f"open run:{run}")
        server, got = marked(page)
        woke, slept = server[0], server[1]
        c(got[0]["text"].startswith((f"第 {woke['data']['n']} 次开工 · ", f"Wake {woke['data']['n']} · "))
          and got[0]["text"].endswith(("· 你让它开工", "· you woke it")),
          f"应用：开工的分隔线写第几次、几点、为什么开工（{got[0]['text']}）")
        c(got[1]["tookMs"] == slept["ts"] - woke["ts"] and ("收工（" in got[1]["text"] or got[1]["text"].startswith("Ended "))
          and ("下次开工：" in got[1]["text"] or "next wake: " in got[1]["text"]),
          f"应用：收工一行写几点收工、用了多久（与服务端 ts 之差相同）和下次开工（{got[1]['text']}）")
        drawn_rows = {r["id"] for r in page["run"]["conversation"]["rows"]}
        c(any(m["seq"] in drawn_rows for m in got), "应用：开工和收工的行画在会话里")
        ctx.app.do("shot loop-session")

    # A question goes to the inbox; the answer wakes the Loop.
    text, err = janus.call("ask_user", {"question": "先修哪个？", "why": "只有你知道优先级", "options": ["导出", "设置"]})
    c(not err, "值班用 ask_user 问用户，不等回答")
    asked = wait(lambda: kinds("question.escalated"), "问题记在值班的会话里", 30)[0]
    if ctx.app:
        s = ctx.app.until(lambda s: any(e["run"] == run and e["kind"] == "question" for e in s["open"]), "问题进了收件箱", "open inbox")
        c(True, "应用：值班的问题在收件箱里")
        # The workspace page lists it under 待处理 as the Loop's row, the sidebar counts it, and no
        # run is listed both there and in the task records.
        mine = [e for e in s["open"] if e["run"] == run]
        home = ctx.app.until(lambda h: any(w["loop"] == loop_id for w in ((h.get("home") or {}).get("waiting") or [])),
                             "工作区页的待处理列出值班", "reload", f"open home:{ws}")
        w = next(w for w in home["home"]["waiting"] if w["loop"] == loop_id)
        c(w["run"] == run and w["items"] == len(mine) and (len(mine) > 1 or w["label"] in ("等你拍板", "Your Call")),
          f"应用：工作区页的待处理有值班这一行，写它在等什么（{w['label']}）")
        here = sum(x["items"] for x in home["home"]["waiting"])
        c(home["sidebar"].get(ws) == here == len([e for e in home["open"] if e["run"] in {x["run"] for x in home["home"]["waiting"]}]),
          f"应用：侧栏的工作区数字算上值班的事，和工作区页一致（{here}）")
        c(not {x["run"] for x in home["home"]["waiting"]} & set(home["home"]["records"]), "应用：待处理的任务不在任务记录里重复列出")
        ctx.app.do("shot loops-waiting")
        ctx.app.until(lambda s: any(i["key"] == asked["data"]["key"] for i in (s.get("run") or {}).get("inbox", [])),
                      "值班页也能回答这个问题", f"open run:{run}")
        c(True, "应用：值班页列出待回答的问题")
        # Answer with the inbox's first quick answer, as the person would.
        s = ctx.app.do("open inbox")
        key = next(e["key"] for e in s["open"] if e["run"] == run and e["kind"] == "question")
        ctx.app.do(f"inbox-act {key}:0")
    else:
        ctx.api("POST", f"/api/runs/{run}/actions", {"type": "answer", "key": asked["data"]["key"], "answer": "导出"})
    # Two steps, so a timeout says which one failed: the answer reaching the server, or it waking the Loop.
    answered = wait(lambda: kinds("question.answered"), "回答到达服务", 30)[0]
    wake_for("answer", answered["seq"], "回答唤醒值班")
    c(True, "回答之后值班以 answer 醒来")
    c(any("问：先修哪个？\n  答：导出" in p for p in prompts()), "值班醒来的提示里带着问题和回答")

    # Paused: no clock, still wakes by hand.
    paused = ctx.api("POST", f"/api/loops/{loop_id}/pause", {})
    c(paused["state"] == "paused" and paused["status"]["phase"] == "paused" and paused["status"]["nextWakeAt"] is None, "暂停后没有下次醒来的时刻")
    if ctx.app:
        ctx.ui(c, run, lambda r, s: any(l["id"] == loop_id and l["phase"] == "paused" for l in s.get("loops", [])),
               "值班页显示已暂停", pages=("board",))
    mark = last_seq()
    ctx.api("POST", f"/api/loops/{loop_id}/wake", {})
    wake_for("manual", mark, "暂停时手动唤醒")
    c(True, "暂停的值班仍可以手动唤醒")
    resumed = ctx.api("POST", f"/api/loops/{loop_id}/resume", {})
    c(resumed["state"] == "active" and resumed["status"]["nextWakeAt"] is not None, "继续后按时间表排下次醒来")

    # The run it dispatched ends: the Loop wakes to judge it, told how it ended.
    mark = last_seq()
    ctx.api("POST", f"/api/runs/{dispatched}/actions", {"type": "cancel"})
    wake_for("dispatch", mark, "派出的任务结束唤醒值班")
    ended = kinds("loop.dispatch_ended")
    c(ended and ended[-1]["data"]["run"] == dispatched and ended[-1]["data"]["status"] == "cancelled",
      "派出的任务被取消后，值班以 dispatch 醒来，记下是哪个任务、怎么结束的")
    c(any(f"- {dispatched}（key github:o/r#7）：被用户取消" in p for p in prompts()), "醒来的提示写着结束的任务和它是被你取消的")

    # Deleting it removes its session; the run it dispatched stays.
    ctx.api("DELETE", f"/api/loops/{loop_id}")
    c(not ctx.api("GET", f"/api/loops?workspace={ws}")["loops"], "删除值班")
    code, _ = ctx.api_error("GET", f"/api/runs/{run}")
    c(code == 404, "值班的会话一起删除")
    c(ctx.api("GET", f"/api/runs/{dispatched}")["run"]["origin"] == "loop", "它派出的任务保留")
