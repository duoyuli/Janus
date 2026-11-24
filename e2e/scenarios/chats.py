"""Chats with one digital employee (docs/domains/20261120-chats.md)."""

import os

from lib import drawn, paths_with, wait
from scenarios import scenario


@scenario("chats")
def chats(ctx, c):
    """单 Agent 对话：点一名数字员工就打开会话（只带数字员工的指令，等用户先说），在自己新建的工作目录里来回发消息，第一条消息作为标题；没有看板，不进任务列表，删除时目录一起删除"""
    t = ctx.team("chats", members=("m1",))
    role = t.member("m1")
    code, _ = ctx.api_error("POST", "/api/chats", {"role": "chats-nobody"})
    c(code == 400, f"不存在的数字员工不能对话（{code}）")

    chat = ctx.api("POST", "/api/chats", {"role": role})["id"]

    def events():
        return ctx.api("GET", f"/api/runs/{chat}")["events"]

    wait(lambda: [e for e in events() if e["type"] == "agent.status" and e["data"].get("turn") == "idle"], "会话打开后等用户先说", 60)
    c("chat session" in t.fake.calls() and not [e for e in events() if e["type"] == "agent.message"],
      "点开就通过 ACP 打开了会话，Agent 还没有说话")
    c("janus-brief" not in t.fake.calls() and not [e for e in events() if e["type"] == "session.prompt" and e["data"].get("channel") == "system"],
      "对话只带数字员工自己的指令，不注入 Janus 的协作说明")
    c(ctx.api("GET", f"/api/runs/{chat}")["run"]["goal"] == "", "第一条消息之前对话没有标题")
    if ctx.app:
        ctx.ui(c, chat, lambda r, s: r.get("chat") and r["status"] == "running", "新对话打开，输入框等用户先说", pages=("density detail",))
    ctx.api("POST", f"/api/runs/{chat}/actions", {"type": "message", "agent": "lead", "text": "你好"})

    def replied(text):
        evs = ctx.api("GET", f"/api/runs/{chat}")["events"]
        # The event's own text is a summary of a long message; the whole of it is in data.text.
        return [e for e in evs if e["type"] == "agent.message" and f"回复：{text}" in e["data"].get("text", e["text"])]

    wait(lambda: replied("你好"), "数字员工回复第一条消息", 60)
    detail = ctx.api("GET", f"/api/runs/{chat}")
    row = detail["run"]
    c(row["kind"] == "chat" and row["status"] == "running", f"对话是 chat 类型的 Run，会话打开着（{row['kind']} / {row['status']}）")
    c(row["goal"] == "你好", "第一条消息成为对话的标题")
    cwd = row["cwd"]
    home = os.path.realpath(ctx.home)
    c(os.path.isdir(cwd) and os.path.realpath(cwd) == os.path.join(home, "work", "chats", chat), f"对话在 Janus home 下自己的目录里，和任务分开：{cwd}")
    c([m["role_id"] for m in row["config"]["team"]] == [role], "团队只有选中的数字员工")
    servers = [line for line in t.fake.calls().splitlines() if line.startswith("mcp ")]
    c(servers and '"name": "janus"' not in servers[-1], "对话的会话没有看板 MCP")
    c(f"chat session {os.path.realpath(cwd)}" in t.fake.calls() or f"chat session {cwd}" in t.fake.calls(), "会话在对话自己的目录里")

    goals = [r["id"] for r in ctx.api("GET", "/api/runs")["runs"]]
    chats_ = [r["id"] for r in ctx.api("GET", "/api/runs?kind=chat")["runs"]]
    c(chat not in goals and chat in chats_, "对话不在任务列表里，在对话列表里")
    code, _ = ctx.api_error("GET", "/api/runs?kind=bogus")
    c(code == 400, f"未知的类型被拒绝（{code}）")
    # Lists come a page at a time, so every chat can be reached however many there are.
    other = ctx.api("POST", "/api/chats", {"role": role})["id"]
    first = ctx.api("GET", "/api/runs?kind=chat&limit=1")
    second = ctx.api("GET", f"/api/runs?kind=chat&limit=1&cursor={first['next']}")
    c([r["id"] for r in first["runs"]] == [other] and [r["id"] for r in second["runs"]] == [chat] and second["next"] is None,
      "对话列表按页读取：一页一个，第二页接着第一页，最后一页没有 next")
    code, _ = ctx.api_error("GET", "/api/runs?kind=chat&cursor=nonsense")
    c(code == 400, f"不对的 cursor 被拒绝（{code}）")
    ctx.api("DELETE", f"/api/runs/{other}")

    ctx.api("POST", f"/api/runs/{chat}/actions", {"type": "message", "agent": "lead", "text": "第二条"})
    wait(lambda: replied("第二条"), "同一个会话回复第二条消息", 60)
    # Only this chat's sessions: the paging check above opened another chat with the same agent.
    mine = [l for l in t.fake.calls().splitlines() if l.startswith("chat session") and chat in l]
    c(len(mine) == 1, f"第二条消息在同一个会话里（这个对话打开过 {len(mine)} 次会话）")

    # A reply in Markdown is drawn as blocks: a table and a code block, not a paragraph of pipes.
    table = "表格\n\n| 仓库 | 分支 |\n|---|---|\n| biz | `feature/p0` |\n\n```bash\ngit push\n```"
    ctx.api("POST", f"/api/runs/{chat}/actions", {"type": "message", "agent": "lead", "text": table})
    wait(lambda: replied(table), "数字员工用 Markdown 回复", 60)
    if ctx.app:
        want = ["para", "table:仓库,分支", "code:bash"]
        st = ctx.app.until(lambda s: (s.get("run") or {}).get("lastReply") == want, "应用把 Markdown 回复画成表格和代码块",
                           f"open run:{chat}", f"shot {chat}-markdown")
        c(st["run"]["lastReply"] == want, f"应用：回复里的表格和代码块按块显示（{st['run']['lastReply']}）")
        c(drawn(ctx.app.picture(f"{chat}-markdown")), f"应用：带表格的回复画出来了（{chat}-markdown.png）")

    # A long reply with a wide table, taller than the window: the table stays inside the message,
    # the rows do not overlap, and the view lands on the newest row instead of the list's estimate
    # of where the end is (which left an empty view).
    long = _long_reply()
    ctx.api("POST", f"/api/runs/{chat}/actions", {"type": "message", "agent": "lead", "text": long})
    wait(lambda: replied(long), "数字员工回复带宽表格的长消息", 60)
    if ctx.app:
        ctx.app.do("size 1000x800")
        st = ctx.app.until(lambda s: (s.get("run") or {}).get("id") == chat and "table:编号,名称,地址,请求,结论" in s["run"]["lastReply"]
                           and _settled(s["run"]["conversation"]), "长回复画完，停在最新一行",
                           f"open run:{chat}", f"shot {chat}-long-reply")
        cv = st["run"]["conversation"]
        rows = cv["rows"]
        # The reply and the status line under it: the rows in view. A row the lazy list keeps aside
        # after it scrolled away may still report where it last was, so older rows are not compared.
        c(len(rows) >= 2 and rows[-1]["top"] >= rows[-2]["bottom"] - 0.5, f"应用：最后的回复和它下面的状态行不重叠（{rows[-2:]}）")
        c(cv["following"] and _settled(cv),
          f"应用：长回复之后停在最新一行，仍在跟随（{cv['following']}，最后一行底边 {rows[-1]['bottom'] if rows else None} / 可见高度 {cv['height']}）")
        c(drawn(ctx.app.picture(f"{chat}-long-reply")), f"应用：带宽表格的长回复画出来了（{chat}-long-reply.png）")

    ctx.ui(c, chat, lambda r, s: r.get("chat") and r["status"] == "running" and any(x["id"] == chat for x in s["chats"]),
           "对话页打开，侧栏的对话列表里有它", pages=("draft lead 第三条", "open chats"))

    if ctx.app:
        # The send key from Settings › 通用, pressed in the chat's composer.
        st = ctx.app.do(f"open run:{chat}", "draft lead 第三条", "press return")
        c(st["sendKey"] == "commandReturn" and _new_line(st) and not replied("第三条"),
          f"应用：默认 ⌘↩ 发送，↩ 只换行（{st['sendKey']} / {st['run']['drafts']}）")
        st = ctx.app.do("send-key return", "open settings:general", "shot settings-send-key", "close-sheet",
                        f"open run:{chat}", "draft lead 第三条", "press shift-return")
        c(st["sendKey"] == "return" and _new_line(st) and not replied("第三条"),
          f"应用：改成 ↩ 发送后，⇧↩ 换行不发送（{st['run']['drafts']}）")
        c(drawn(ctx.app.picture("settings-send-key")), "应用：设置 › 通用里的「发送消息」画出来了（settings-send-key.png）")
        # Scenarios share the app: each batch opens the chat itself, since another one may have
        # opened its own run between batches.
        ctx.app.do(f"open run:{chat}", "draft lead 第三条", "press return")
        wait(lambda: replied("第三条"), "按 ↩ 发出的消息得到回复", 60)
        st = ctx.app.do("send-key commandReturn", f"open run:{chat}")
        c(st["run"]["drafts"].get("sess-lead", "") == "", "应用：按 ↩ 把第三条发给了数字员工，输入框清空")

    with open(os.path.join(cwd, "notes.md"), "w") as f:
        f.write("scratch\n")
    ctx.api("DELETE", f"/api/runs/{chat}")
    left = paths_with(ctx.home, chat)
    c(not os.path.exists(cwd) and not left, f"删除对话时它的工作目录、资源包和日志都删除：{left}")
    code, _ = ctx.api_error("GET", f"/api/runs/{chat}")
    c(code == 404, f"删除后对话不存在（{code}）")


def _long_reply():
    """A reply like an agent's report: a five-column table whose cells are too long for one line,
    then lists and paragraphs, taller than the window."""
    rows = "\n".join(f"| {n} | 第 {n} 个服务（北区 0.{n}） | service-{n}.example.invalid | {n * 37} | "
                     f"全部 `model-{n}-long-name`，另有 {n} 条 `relay/model-{n}-long-name`（mismatch=true） |" for n in range(1, 4))
    items = "\n".join(f"- 第 {n} 项：`/api/v1/admin/ops/item-{n}?filter=all&page=1` 记录了这一项的请求和结论，按时间顺序列出。" for n in range(1, 9))
    return (f"结论先说：**三个服务都正常返回，没有出现别的模型。**\n\n## 调用情况\n\n| 编号 | 名称 | 地址 | 请求 | 结论 |\n|---|---|---|---|---|\n{rows}\n\n"
            f"## 依据\n\n{items}\n\n## 顺带发现\n\n1. **配置矛盾**：一项回退规则指向不允许的协议，触发时会被拒绝。\n2. **刷屏**：一个不支持的端点被反复调用。\n\n今天没有触发告警。")


def _settled(cv):
    """The newest row is wholly in view at the bottom of the conversation: no further from it than
    the app's own idea of being at the end (`ChatFollow.slack`, 32 points)."""
    rows = cv.get("rows") or []
    return bool(rows) and 0 < rows[-1]["bottom"] <= cv["height"] and cv["height"] - rows[-1]["bottom"] <= 32


def _new_line(st):
    """The chat's draft is 第三条 with a new line added where the caret was."""
    draft = st["run"]["drafts"].get("sess-lead", "")
    return draft.count("\n") == 1 and draft.replace("\n", "") == "第三条"
