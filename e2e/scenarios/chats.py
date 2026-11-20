"""Chats with one digital employee (docs/domains/20261120-chats.md)."""

import os

from lib import wait
from scenarios import scenario


@scenario("chats")
def chats(ctx, c):
    """单 Agent 对话：选一名数字员工开始对话，在自己新建的工作目录里来回发消息，没有看板，不进任务列表，删除时目录一起删除"""
    t = ctx.team("chats", members=("m1",))
    role = t.member("m1")
    code, _ = ctx.api_error("POST", "/api/chats", {"role": role, "text": "   "})
    c(code == 400, f"空消息不能开始对话（{code}）")
    code, _ = ctx.api_error("POST", "/api/chats", {"role": "chats-nobody", "text": "你好"})
    c(code == 400, f"不存在的数字员工不能对话（{code}）")

    chat = ctx.api("POST", "/api/chats", {"role": role, "text": "你好"})["id"]

    def replied(text):
        evs = ctx.api("GET", f"/api/runs/{chat}")["events"]
        return [e for e in evs if e["type"] == "agent.message" and f"回复：{text}" in e["text"]]

    wait(lambda: replied("你好"), "数字员工回复第一条消息", 60)
    detail = ctx.api("GET", f"/api/runs/{chat}")
    row = detail["run"]
    c(row["kind"] == "chat" and row["status"] == "running", f"对话是 chat 类型的 Run，会话打开着（{row['kind']} / {row['status']}）")
    cwd = row["cwd"]
    home = os.path.realpath(ctx.home)
    c(os.path.isdir(cwd) and os.path.realpath(cwd) == os.path.join(home, "chats", chat), f"对话在 Janus home 下自己的目录里：{cwd}")
    c([m["role_id"] for m in row["config"]["team"]] == [role], "团队只有选中的数字员工")
    servers = [line for line in t.fake.calls().splitlines() if line.startswith("mcp ")]
    c(servers and '"name": "janus"' not in servers[-1], "对话的会话没有看板 MCP")
    c(f"chat brief {os.path.realpath(cwd)}" in t.fake.calls() or f"chat brief {cwd}" in t.fake.calls(), "Agent 知道自己在单独对话、工作目录在哪")

    goals = [r["id"] for r in ctx.api("GET", "/api/runs")["runs"]]
    chats_ = [r["id"] for r in ctx.api("GET", "/api/runs?kind=chat")["runs"]]
    c(chat not in goals and chat in chats_, "对话不在任务列表里，在对话列表里")
    code, _ = ctx.api_error("GET", "/api/runs?kind=bogus")
    c(code == 400, f"未知的类型被拒绝（{code}）")

    ctx.api("POST", f"/api/runs/{chat}/actions", {"type": "message", "agent": "lead", "text": "第二条"})
    wait(lambda: replied("第二条"), "同一个会话回复第二条消息", 60)
    c(t.fake.calls().count("chat brief") == 1, "第二条消息在同一个会话里")

    ctx.ui(c, chat, lambda r, s: r.get("chat") and r["status"] == "running" and any(x["id"] == chat for x in s["chats"]),
           "对话页打开，侧栏的对话列表里有它", pages=("density normal", "open chats"))

    with open(os.path.join(cwd, "notes.md"), "w") as f:
        f.write("scratch\n")
    ctx.api("DELETE", f"/api/runs/{chat}")
    c(not os.path.exists(cwd), "删除对话时工作目录一起删除")
    code, _ = ctx.api_error("GET", f"/api/runs/{chat}")
    c(code == 404, f"删除后对话不存在（{code}）")
