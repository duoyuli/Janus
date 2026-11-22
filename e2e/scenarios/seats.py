"""Agent seats: how many agent processes are open at once, and idle ones giving theirs back
(docs/domains/20261031-run_lifecycle.md#agent_seats)."""

from lib import wait
from scenarios import scenario


@scenario("seats", serial=True)
def seats(ctx, c):
    """Agent 进程的上限：两个位置都被对话占着时，第三个对话等空位，闲得最久的对话让出位置、在应用里显示已关闭，给它发消息又会重新打开"""
    # Two seats (one working goal run plus one), on a fresh start.
    ctx.server.env.update({"JANUS_MAX_RUNNING_RUNS": "1", "JANUS_MAX_AGENT_SESSIONS": "2"})
    ctx.server.stop()
    ctx.server.start()
    try:
        t = ctx.team("seats", members=("m1",))
        role = t.member("m1")

        def events(run):
            return ctx.api("GET", f"/api/runs/{run}")["events"]

        def replied(run, text):
            return [e for e in events(run) if e["type"] == "agent.message" and f"回复：{text}" in e["text"]]

        def chat(text):
            run = ctx.api("POST", "/api/chats", {"role": role})["id"]
            ctx.api("POST", f"/api/runs/{run}/actions", {"type": "message", "agent": "lead", "text": text})
            wait(lambda: replied(run, text), f"对话回复「{text}」", 60)
            return run

        first = chat("一")
        second = chat("二")
        third = chat("三")
        parked = [e for e in events(first) if e["type"] == "session.parked"]
        c(parked and parked[0]["data"]["reason"] == "full", "两个位置都占着时，闲得最久的第一个对话让出位置")
        c(not [e for e in events(second) if e["type"] == "session.parked"], "只让出需要的那一个")
        c([e for e in events(third) if e["type"] == "agent.status" and e["data"].get("phase") == "waiting_for_seat"],
          "第三个对话先等空位，并写明在等")
        wait(lambda: ctx.api("GET", f"/api/runs/{first}")["run"]["status"] == "interrupted", "第一个对话记为已关闭", 30)
        if ctx.app:
            st = ctx.app.until(lambda s: next((r["status"] for r in s.get("chats", []) if r["id"] == first), None) == "interrupted",
                               "应用里第一个对话显示已关闭", "reload")
            ctx.app.do("open chats", "shot seats-chats")
            c(True, "应用：让出位置的对话在列表里是已关闭（seats-chats.png）")
        ctx.api("POST", f"/api/runs/{first}/actions", {"type": "message", "agent": "lead", "text": "回来"})
        wait(lambda: replied(first, "回来"), "让出位置的对话再收到消息时重新打开并回复", 60)
        c(ctx.api("GET", f"/api/runs/{first}")["run"]["status"] == "running", "重新打开的对话又在会话中")
        for run in (first, second, third):
            ctx.api("DELETE", f"/api/runs/{run}")
    finally:
        ctx.server.env["JANUS_MAX_RUNNING_RUNS"] = "32"
        ctx.server.env.pop("JANUS_MAX_AGENT_SESSIONS", None)
        ctx.server.stop()
        ctx.server.start()
