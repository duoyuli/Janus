"""The server going away mid-run: what is kept, what is interrupted, and how work continues."""

import json
import threading

from lib import drawn, wait
from scenarios import scenario, task


@scenario("restart", own=True)
def restart(ctx, c):
    """服务重启：进行中的 Run 记为中断后自动从中断处继续，暂停的 Run 等你继续，看板和痕迹恢复，负责人恢复原来的会话并收到重启时的看板；已交付的 Run 回放时会话只读，重启后还能继续对话，同时发来的两条消息只恢复一次会话；恢复的会话拿到和原来一样的系统层，不再注入历史和第二份指令；应用重新连上"""
    # A server of its own: this scenario stops and starts it.
    with ctx.private("restart") as ctx:
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
        # Paused by the person before the restart: it waits for them afterwards.
        p = ctx.team("restart-paused", members=("m1",))
        paused = p.run("暂停着跨过重启的目标")
        paused.wait_status("running")
        paused.act("pause")
        paused.wait_event("run.paused")
        h = ctx.team("restart-chat", members=("m1",))
        chat = ctx.api("POST", "/api/chats", {"role": h.member("m1")})["id"]
        ctx.api("POST", f"/api/runs/{chat}/actions", {"type": "message", "agent": "lead", "text": "重启前说的话"})

        def chat_events():
            return ctx.api("GET", f"/api/runs/{chat}")["events"]

        def chat_replied(text):
            return [e for e in chat_events() if e["type"] == "agent.message" and f"回复：{text}" in e["text"]]

        wait(lambda: chat_replied("重启前说的话"), "对话在重启前回复了一条", 60)

        ctx.server.stop()
        meta = ctx.server.start()
        c(meta["home"] == ctx.home, "服务在同一个 Janus home 上重新启动")
        interrupted = run.wait_event("run.interrupted")
        c(interrupted["data"].get("after") == "goes_on", "重启时没结束的 Run 记为中断，写明服务启动后自动继续")
        c(done.status() == "delivered", "已交付的 Run 不受影响")

        # The person paused this one: it waits for them, and nothing that needs a running run works.
        paused.wait_status("interrupted")
        c(paused.has("run.interrupted")[-1]["data"].get("after") == "paused", "重启前暂停的 Run 不自动继续，等你决定")
        c(not ctx.api("GET", f"/api/runs/{paused.id}")["live"], "等你继续的 Run 不在内存中运行")
        code, _ = paused.act_error("pause")
        c(code == 409, "中断的 Run 不能执行要求它正在运行的操作（409）")
        ctx.ui(c, paused.id, lambda r, s: r["status"] == "interrupted", "应用重新连上服务，显示暂停的 Run 已中断", pages=("board",))
        paused.act("resumeRun")
        resumed = paused.wait_event("run.recovered")
        c(resumed["actor"] == "user" and not resumed["data"].get("auto"), "你点「从中断处继续」后它接着做")
        paused.finish("继续后完成")

        # Nobody touched the working one: it went on by itself, with its lead's own session.
        lead_sid = next(e["data"]["session"] for e in run.events() if e["actor"] == "lead" and (e.get("data") or {}).get("session"))
        recovered = run.wait_event("run.recovered")
        c(recovered["actor"] == "janus" and recovered["data"].get("auto") is True, "没人操作，服务启动后自动从中断处继续")
        wait(lambda: len([u for u in t.fake.urls(run.id) if u.endswith("/lead")]) >= 2, "负责人会话以新的看板地址重新打开")
        c(True, "恢复后的负责人会话拿到新的看板地址（旧令牌作废）")
        # The lead's own session comes back with everything it did; it is told what the restart
        # changed instead of being given the goal again.
        t.fake.wait_call(f"lead resumed {lead_sid} ")
        calls = t.fake.calls()
        c(f"load {lead_sid}" in calls and calls.count("goal 跨越重启的目标") == 1,
          "负责人恢复原来的会话（session/load），没有重发 /goal")
        told = next(line for line in calls.splitlines() if line.startswith(f"lead resumed {lead_sid} "))
        c("T1 (" in told and "· 已完成" in told and "T2 (" in told and "Janus 停下时它正在工作" in told,
          "恢复提示列出重启时的看板：T1 已完成，T2 正在执行、接回自己的会话继续")
        c(any(e["type"] == "session.prompt" and e["data"].get("channel") == "resume" and e["data"].get("session") == lead_sid
              for e in run.events()), "恢复提示记在负责人原来的会话里（channel resume）")
        c(not [e for e in run.events() if e["seq"] > run.has("run.recovered")[-1]["seq"] and e["type"] == "session.prompt"
               and e["actor"] == "janus" and e.get("to") == "lead" and e["data"].get("via")],
          "恢复的负责人会话没有再注入负责人说明和指令")
        c(run.id in ctx.api("GET", "/api/meta")["atWork"], "服务报告这个 Run 在干活（/api/meta 的 atWork），应用自动更新会等它")
        ctx.ui(c, run.id, lambda r, s: r["status"] == "running", "恢复后的负责人会话显示恢复提示", pages=("agent:lead",))
        board = run.read_board(lambda b: "[完成] T3" in b, timeout=90)
        c("[完成] T2" in board, "中断的 T2 重新执行并完成，下游 T3 随后完成")
        c(not [e for e in run.has("attempt.started") if e.get("task") == "T1"][1:], "已完成的 T1 不重跑")
        lead = run.lead()
        detail, _ = lead.call("board", {"task": "T1"})
        c("a.txt" in detail, "重启后 T1 改过的文件（痕迹）仍在")
        run.finish("重启后也完成了")
        c(run.status() == "delivered", "恢复的 Run 交付")
        wait(lambda: run.id not in ctx.api("GET", "/api/meta")["atWork"], "交付后负责人在等人，不再算在干活")
        c(True, "交付后负责人等你的消息，atWork 里不再有它，应用可以自动更新")

        # Delivered before the restart, it is no longer in the service's memory and still waits for
        # the person: the card offers acceptance and the page is not marked read-only.
        st = ctx.ui(c, done.id, lambda r, s: r["status"] == "delivered" and r["review"], "重启前交付的 Run 在应用中已交付")
        if st:
            rv = st["run"]["review"]
            c(not st["run"]["live"] and rv["stage"] == "awaiting" and rv["actions"] and not st["run"]["readOnly"],
              f"应用：重启前交付、不在服务内存里的 Run 仍等你验收，给出「验收」和「让负责人改…」，不标只读"
              f"（live {st['run']['live']}，阶段 {rv['stage']}，按钮 {rv['actions']}，只读 {st['run']['readOnly']}）")
            # Replaying it shows the lead's session read-only: a draft typed there stays and is not sent.
            shot = f"{done.id}-replay-draft"
            ctx.app.do(f"open run:{done.id}", "page replay:3", "draft agent:lead 回放时写的草稿", f"shot {shot}")
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
        c(sum(line.startswith("load ") for line in d.fake.calls().splitlines()) == 1, "两条消息同时发给只有事件记录的历史 Run，只加载一次原会话（session/load）")

        def reached(m):
            mid = m["data"]["id"]
            return any(e["type"] == "message.routed" and e["data"].get("id") == mid and e["data"].get("delivery") in ("steer", "prompt")
                       or e["type"] == "message.delivered" and mid in e["data"].get("ids", []) for e in done.events())
        wait(lambda: all(reached(m) for m in said), "两条消息都送到负责人")
        c(True, "同时发来的两条消息都送到恢复的负责人会话")
        kept_layer(c, d.fake.calls(), "重启前交付的 Run")
        c(not [e for e in done.events() if e["seq"] > sent and e["type"] == "session.prompt" and e["data"].get("via")],
          "恢复的负责人会话没有再注入任何系统层内容（没有交付摘要、之前的对话和第二份指令）")

        # A chat opened before the restart: the next message loads its session, which hears nothing again.
        before = max(e["seq"] for e in chat_events())
        ctx.api("POST", f"/api/runs/{chat}/actions", {"type": "message", "agent": "lead", "text": "重启后还在吗"})
        wait(lambda: chat_replied("重启后还在吗"), "重新打开的对话回复", 60)
        kept_layer(c, h.fake.calls(), "重启前的对话")
        c(not [e for e in chat_events() if e["seq"] > before and e["type"] == "session.prompt" and e["data"].get("via")],
          "恢复的对话没有注入之前的对话，也没有第二份数字员工的指令")
        ctx.ui(c, chat, lambda r, s: r.get("chat") and r["status"] == "running", "重启后重新打开的对话", pages=("density detail",))
        ctx.ui(c, run.id, lambda r, s: r["status"] == "delivered" and all(x["status"] == "succeeded" for x in r["tasks"]),
               "恢复后的 Run 在应用中已交付，任务都已完成", pages=("board", "audit"))


def kept_layer(c, calls, what):
    """The stand-in agent logs the system layer of each `session/new` and `session/load`: a loaded
    session must get exactly the one it was opened with."""
    layers = [line.split(" ", 3) for line in calls.splitlines() if line.startswith("layer ")]
    loads = [(sid, json.loads(text)) for _, method, sid, text in layers if method == "load"]
    opened = {sid: json.loads(text) for _, method, sid, text in layers if method == "new"}
    c(len(loads) == 1 and loads[0][0] in opened and loads[0][1] == opened[loads[0][0]],
      f"{what}：恢复原会话（session/load）时交回的系统层和打开时完全一样")
