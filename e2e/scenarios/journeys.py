"""The main journeys: a goal from submission to acceptance, with each built-in lead."""

import base64
import json
import os
import socket

from lib import Mcp, wait
from scenarios import jsonl, read_file, scenario, task

# A 1×1 PNG: what a pasted screenshot looks like to Janus.
PNG = base64.b64encode(bytes.fromhex(
    "89504e470d0a1a0a0000000d4948445200000001000000010806000000"
    "1f15c4890000000d49444154789c6360f8cf00000301010018dd8db4000000"
    "0049454e44ae426082")).decode()


@scenario("goal-codex")
def goal_codex(ctx, c):
    """目标全流程（Codex 负责人）：编排校验、看板令牌、进展与提问、交付、产物、导出、事件流、验收后继续"""
    t = ctx.team("goal-codex", members=("m1", "m2"), script={"T1": ["write:notes.md"], "T2": ["ask", "ok"]})
    m1, m2 = t.member("m1"), t.member("m2")
    run = t.run("写一份笔记", images=[{"data": PNG, "name": "草图.png", "mimeType": "image/png"}])
    lead = run.lead()
    system = [e for e in run.events() if e["type"] == "session.prompt" and e.get("to") == "lead" and e["data"].get("channel") == "system"]
    brief = json.dumps(system[:1], ensure_ascii=False)
    c("janus.orchestrate" in brief, "负责人说明在系统层注入，先于 /goal")
    c("[Image 1]" in brief and "/attachments/" in brief, "随目标粘贴的图片以路径写进负责人说明")
    c(len(lead.tools()) == 12, "负责人有 10 个看板工具和团队记忆的 2 个工具")

    # Validation first: every problem at once, and nothing starts.
    text, err = lead.call("orchestrate", {"tasks": [
        {"id": "T1", "title": "写笔记", "instructions": "写", "assignee": "nobody"},
        {"id": "T2", "title": "定名", "instructions": "定", "assignee": m2, "deps": ["T9"]}]})
    c(err and "nobody" in text and "T9" in text and "accept" in text, "不合格的编排一次列出全部问题")
    c(run.has("orchestration.rejected") and not run.has("task.assigned"), "被拒绝的编排一个任务都不启动")
    run.orchestrate(task("T1", m1, instructions="在 notes.md 写笔记", owns=["notes.md"]),
                    task("T2", m2, deps=["T1"], instructions="给笔记定个名字"))
    text, err = lead.call("orchestrate", {"tasks": [task("T1", m1)]})
    c(err and "重复" in text, "任务 id 在多次编排之间不能重复")

    # Each session's board URL is bound to its owner.
    t1 = t.fake.board(run.id, "T1")
    c(t1.tools() == ["board_post", "claim", "peers", "request_split", "recall", "remember"], "协作者只有自己的 4 个工具和团队记忆的读写")
    text, err = t1.call("board_post", {"kind": "progress", "text": "笔记写了一半"})
    c(not err, "协作者写回进展")
    c(Mcp(t1.url.rsplit("/", 1)[0] + "/lead").post({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})[0] == 403,
      "协作者把 URL 改成 lead 被拒绝（403）")
    c(Mcp(t1.url.rsplit("/", 1)[0] + "/T2").post({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})[0] == 403,
      "协作者冒充其他任务被拒绝（403）")
    c(Mcp(lead.url.replace(lead.url.split("/")[-2], "0" * 32)).post({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})[0] == 403,
      "错误的令牌被拒绝（403）")
    status, _ = Mcp(lead.url).post({"jsonrpc": "2.0", "method": "notifications/initialized"})
    c(status == 202, "MCP 通知返回 202")

    answered = []
    def answer(board):
        if not answered and "[提问] T2" in board:
            text, err = lead.call("reply", {"task": "T2", "text": "alpha"})
            answered.append(not err and "已回答" in text)
    board = run.read_board(lambda b: "[完成] T2" in b, on=answer)
    c(answered == [True], "负责人在看板上回答协作者的提问")
    c("[进展] T1" in board and "[完成] T1" in board and "按回答 alpha 完成" in board, "回答回到 T2 的会话，T2 按回答完成")
    c(not run.has("question.escalated"), "协作者的提问先到负责人，不打扰用户")
    detail, err = lead.call("task_detail", {"task": "T1"})
    c(not err and "notes.md" in detail, "task_detail 列出 T1 改过的文件")

    delivered = run.finish("目标已完成：笔记写好了")
    c("笔记写好了" in delivered["data"]["summary"], "交付摘要来自负责人的最后回复")
    c(read_file(f"{run.cwd()}/notes.md") == "T1\n" and not os.path.exists(f"{t.repo}/notes.md"),
      "T1 在这个任务单独的副本里写了文件，工作区目录不受影响")

    # What the run left: artifacts, the readable diff, the export and the live stream.
    arts = run.has("artifact.created", task="T1")
    diff = next((a for a in arts if a["data"]["kind"] == "git_diff"), None)
    c(diff, "T1 保存了 diff 产物")
    if diff:
        status, body, _ = ctx.api("GET", f"/api/file?path={diff['data']['path']}", raw=True)
        c(status == 200 and b"notes.md" in body, "产物可以通过 /api/file 读取")
    code, _ = ctx.api_error("GET", f"/api/file?path={ctx.home}/data/janus.db")
    c(code == 403, "/api/file 不能读 Janus home 中产物以外的文件")
    before = [e["seq"] for e in run.events()]
    status, body, headers = ctx.api("GET", f"/api/runs/{run.id}/export", raw=True)
    seqs = [e["seq"] for e in jsonl(body.decode())]
    c(seqs[:len(before)] == before and "ndjson" in {k.lower(): v for k, v in headers.items()}.get("content-type", ""), f"导出的 NDJSON 与 Run 详情的事件一致（{len(seqs)} 条）")
    c(_sse(ctx, run.id), "SSE 事件流先发送历史事件")
    usage = ctx.api("GET", f"/api/runs/{run.id}/usage")
    c(usage["run"]["sessions"] >= 3 and usage["text"], f"用量账本：{usage['text']}")

    code, _ = run.act_error("accept", expectedSequence=1)
    c(code == 409, "带过期 expectedSequence 的操作返回 409")
    run.act("accept", commandId="accept-1")
    dup = run.act("accept", commandId="accept-1")
    c(dup.get("duplicate") is True, "同一个 commandId 只执行一次")
    run.wait_status("accepted")
    c(True, "验收后 Run 是已验收")
    code, _ = ctx.api_error("DELETE", f"/api/runs/{run.id}x")
    c(code == 404, "删除不存在的 Run 返回 404")

    # An accepted goal still takes instructions: the lead's session answers.
    run.act("message", text="再检查一遍笔记", agent="lead")
    sent = run.wait_event("user.message")["seq"]
    wait(lambda: [e for e in run.events() if e["type"] == "agent.message" and e["actor"] == "lead" and e["seq"] > sent],
         "负责人回复验收后的消息")
    c(True, "验收后给负责人发消息，负责人照常回复")
    ctx.ui(c, run.id, lambda r, s: {x["id"]: x["status"] for x in r["tasks"]} == {"T1": "succeeded", "T2": "succeeded"}
           and r["delivered"] and r["accepted"], "T1、T2 已完成，Run 已交付并验收",
           pages=("board", "team", "audit", "task:T2", "agent:lead"))


def _sse(ctx, run):
    """The first message of the stream is the run's first event."""
    host, port = ctx.addr.split(":")
    s = socket.create_connection((host, int(port)), timeout=10)
    s.sendall(f"GET /api/runs/{run}/stream?after=0 HTTP/1.1\r\nHost: {ctx.addr}\r\nAccept: text/event-stream\r\n\r\n".encode())
    buf = b""
    while not (b"data:" in buf and b"\n\n" in buf.split(b"data:", 1)[1]):
        chunk = s.recv(65536)
        if not chunk:
            break
        buf += chunk
    s.close()
    data = buf.split(b"data:", 1)[-1].split(b"\n", 1)[0].strip()
    try:
        return json.loads(data)["seq"] == 1
    except (ValueError, KeyError):
        return False


@scenario("goal-claude")
def goal_claude(ctx, c):
    """Claude Code 负责人：看板消息推送进会话、交付后新的 /goal（期间提问仍归负责人）、随目标和消息粘贴的图片"""
    t = ctx.team("goal-claude", lead="claude", members=("m1",), script={"T1": ["ask", "ok"], "T2": ["ask", "ok"]}, fake_files=("steer",))
    m1 = t.member("m1")
    run = t.run("按截图修好页面", images=[{"data": PNG, "name": "截图.png", "mimeType": "image/png"}])
    lead = run.lead()
    created = run.wait_event("run.created")
    c(len(created["data"]["images"]) == 1 and created["data"]["images"][0]["path"].startswith(ctx.home), "目标的图片保存在 Janus home 中这个 Run 的目录")
    run.orchestrate(task("T1", m1))
    t.fake.wait_call("steer ", 30)
    c("[提问] T1" in t.fake.calls().split("steer ", 1)[1], "协作者的提问推送进支持插话的负责人会话（_session/steering）")
    run.wait_event("message.delivered", data__applied=True)
    c(True, "插话被读到后记为送达")
    text, _ = lead.call("wait", {"timeout_sec": 1})
    c("[提问] T1" not in text, "推送过的消息不再由 wait 返回")
    lead.call("reply", {"task": "T1", "text": "beta"})
    run.wait_event("task.succeeded", task="T1")
    run.finish("页面修好了")
    c(run.status() == "delivered", "第一个目标交付")

    t.fake.remove("lead-go")
    run.act("message", text="/goal 再加一个页脚", agent="lead")
    t.fake.wait_call("goal 再加一个页脚")
    c(True, "交付后负责人会话接受新的 /goal")
    run.orchestrate(task("T2", m1))
    t.fake.wait_call("[提问] T2", 30)
    c(not run.has("question.escalated"), "交付后负责人推进新目标时，协作者的提问推送给负责人，不转给用户")
    lead.call("reply", {"task": "T2", "text": "alpha"})
    run.wait_event("task.succeeded", task="T2")
    run.act("message", text="页脚参考这张图", agent="lead", images=[{"data": PNG, "name": "页脚.png", "mimeType": "image/png"}])
    t.fake.wait_call("steer 页脚参考这张图")
    c("[Image 1]" in t.fake.calls().split("steer 页脚参考这张图", 1)[1].split("\n", 1)[0], "消息里粘贴的图片随插话进入进行中的目标")
    t.fake.finish_lead("页脚也加好了")
    wait(lambda: len(run.has("run.delivered")) >= 2, "第二次交付")
    c(True, "新的目标完成后再次交付")
    code, text = ctx.api_error("POST", "/api/runs", {"goal": "x", "workspace": t.ws["id"], "images": [{"data": "bm90IGFuIGltYWdl", "name": "a.png"}]})
    c(code == 400, f"不是图片的附件被拒绝（{text.strip()[:60]}）")
    ctx.ui(c, run.id, lambda r, s: r["status"] == "delivered" and {x["id"] for x in r["tasks"]} == {"T1", "T2"},
           "两个目标的任务都在，Run 已交付", pages=("board", "agent:lead"))


@scenario("usage")
def usage(ctx, c):
    """用量账本：按模型上报、只报上下文、多轮相加；接口、账单与应用的数字一致"""
    t = ctx.team("usage", members=("m1", "m2", "m3"), script={"T1": ["usage"], "T2": ["usage-quiet"], "T3": ["usage-turns"]})
    run = t.run("统计用量")
    run.orchestrate(task("T1", t.member("m1")), task("T2", t.member("m2")), task("T3", t.member("m3")))
    lead = run.lead()
    def answer(board):
        if "[提问] T3" in board and not run.has("question.answered", task="T3"):
            lead.call("reply", {"task": "T3", "text": "alpha"})
    run.read_board(lambda b: all(f"[完成] {x}" in b for x in ("T1", "T2", "T3")), on=answer)
    run.finish("用量都记下了")
    ledger = ctx.api("GET", f"/api/runs/{run.id}/usage")
    by_task = {x.get("id") or x.get("task"): x for x in ledger["tasks"]}
    total = lambda x: (x.get("tokens") or {}).get("total")
    c(total(by_task.get("T1", {})) == 2000, f"T1 按模型上报：主模型 1500 加子 Agent 500（{total(by_task.get('T1', {}))}）")
    c(total(by_task.get("T2", {})) is None, f"T2 只报上下文，token 记为未上报（{total(by_task.get('T2', {}))}）")
    c(total(by_task.get("T3", {})) == 3500, f"T3 两轮相加 1000 + 2500（{total(by_task.get('T3', {}))}）")
    c(ledger["run"]["completeness"] == "partial", "有会话没上报时 Run 的用量标为部分上报")
    cost = ledger["run"]["cost"]["lines"]
    c(cost and cost[0]["amount"] == 0.25 and cost[0]["currency"] == "USD", f"费用取 Agent 上报的 0.25 USD：{cost}")
    import time as _t
    now = int(_t.time() * 1000)
    bill = ctx.api("GET", f"/api/usage?from={now - 3600_000}&to={now + 60_000}&offset=480")
    c(bill["runs"] >= 1 and (bill["tokens"]["total"] or 0) >= 5500, f"时间段账单包括这个 Run（{bill['runs']} 个 Run，{bill['tokens']['total']} tokens）")
    want = ledger["run"]["tokens"]["total"]
    ctx.ui(c, run.id, lambda r, s: r.get("tokens") == want, f"应用算出的 Run 用量与服务一致（{want} tokens）",
           pages=("fold usage open", "task:T1"))
    if ctx.app:
        ctx.app.do("open settings:usage", "shot usage-bill", "close-sheet")
