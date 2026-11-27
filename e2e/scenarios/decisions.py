"""Decisions that reach the person: approvals, forms, the lead's questions, and questions left behind."""

import time

from lib import Failure, drawn, wait
from scenarios import scenario, task


@scenario("decisions")
def decisions(ctx, c):
    """审批、表单、负责人 ask_user 和负责人留下的提问都到用户，在应用收件箱里就地处理"""
    t = ctx.team("decisions", members=("m1", "m2", "m3"), script={"T1": ["permission"], "T2": ["elicit"], "T3": ["ask", "ok"]})
    run = t.run("需要用户决定的目标")
    lead = run.lead()
    run.orchestrate(task("T1", t.member("m1")), task("T2", t.member("m2")))
    text, err = lead.call("ask_user", {"question": "部署到哪个环境？", "why": "需要用户的账号", "options": ["测试", "生产"], "recommend": "测试"})
    c(not err, "负责人用 ask_user 把决定交给用户")
    approval = run.wait_event("approval.requested", task="T1")
    form = run.wait_event("input.requested", task="T2")
    asked = run.wait_event("question.escalated", actor="lead")
    c(len(approval["data"]["options"]) == 3, "审批请求带着 Agent 的三个选项")
    c(form["data"]["schema"]["properties"]["name"]["type"] == "string", "表单请求带着 JSON Schema")
    c(run.status() == "running", "等待用户时 Run 仍在执行")

    if ctx.app:
        # T1's approval and T2's form keep the run waiting on the person, and the scripted agents
        # write no events meanwhile: a card that only counted events would stand still here. This
        # runs before the inbox state below is captured, so the approval click is not delayed.
        opened = ctx.app.until(lambda s: (s.get("run") or {}).get("id") == run.id and s["run"]["loaded"],
                               "应用打开这个 Run", f"open run:{run.id}")
        first = opened["run"]
        time.sleep(3)
        second = ctx.app.do(f"open run:{run.id}", "page board", f"shot {run.id}-elapsed")["run"]
        c(first["live"] and second["live"] and first["status"] == second["status"] == "running",
          "按秒推进：等待用户时两次读取都是实时、执行中的 Run")
        c(first["events"] == second["events"],
          f"两次读取之间没有新事件（{first['events']} 条），耗时仍在推进")
        a, b = first.get("elapsedMs"), second.get("elapsedMs")
        c(a is not None and b is not None and b - a >= 2000, f"应用：没有新事件时，群标题下的耗时仍按秒推进（{a} → {b} ms）")
        c(drawn(ctx.app.picture(f"{run.id}-elapsed")), f"应用：群聊页画出来了（{run.id}-elapsed.png）")
        st = ctx.app.until(lambda s: len([e for e in s["open"] if e["run"] == run.id]) >= 3, "收件箱列出三个等用户处理的事项",
                           "open inbox", timeout=60)
        mine = [e for e in st["open"] if e["run"] == run.id]
        c(True, f"应用：收件箱有 {len(mine)} 个事项：{[e['kind'] for e in mine]}")
        ctx.app.do("open inbox", "shot inbox")
        blocking = [e for e in mine if not e["canWait"]]
        c(len(blocking) >= 2, "会卡住 Agent 的审批和表单不能推迟")
        # Approve in the app, through the same code as a click on the inbox's quick answer. The
        # parallel scenarios deliver meanwhile and reorder the inbox, so the entry goes by its key.
        fresh = ctx.app.do("open inbox")
        e = next((e for e in fresh["open"] if e["run"] == run.id and e["kind"] == "approval"), None)
        if e is None:
            raise Failure(f"收件箱里找不到这个 Run 的审批事项：{[e['kind'] for e in fresh['open'] if e['run'] == run.id]}")
        c(True, f"应用：审批的就地按钮 {e['actions']}")
        ctx.app.do(f"inbox-act {e['key']}:0")
        granted = run.wait_event("approval.granted", task="T1", timeout=30)
        c(granted, "应用收件箱里点的审批到达服务")
    else:
        run.act("approve", key=approval["data"]["key"], task="T1", allow=True, option="a1", label="Allow once")
    summary = run.wait_event("task.succeeded", task="T1")["data"]["summary"]
    c(summary.startswith("picked a"), f"Agent 收到所选的审批项（{summary}）")

    run.act("input", key=form["data"]["key"], task="T2", action="accept", content={"name": "e2e-name"})
    c(run.wait_event("task.succeeded", task="T2")["data"]["summary"] == "accept e2e-name", "表单的内容回到 Agent")
    code, text = run.act_error("input", key=form["data"]["key"], task="T2", action="accept", content={"name": "x"})
    c(code == 400 and "没有等待中的决定" in text, "已经处理过的请求不能再处理")
    run.act("answer", key=asked["data"]["key"], task="", answer="测试")
    board = run.read_board(lambda b: "[用户决定]" in b)
    c("测试" in board.split("[用户决定]", 1)[1], "用户对 ask_user 的回答以「用户决定」回到负责人")

    # A question the lead leaves behind goes to the person, and the run still finishes.
    run.orchestrate(task("T3", t.member("m3")))
    run.wait_event("question.asked", task="T3")
    # A question the lead hands over and then delivers without: nobody reads an answer to it any more.
    text, err = lead.call("ask_user", {"question": "要不要顺便发版？", "why": "发版要用户的账号"})
    c(not err, "负责人又交出一个问题，没等回答就交付")
    unanswered = run.wait_event("question.escalated", actor="lead", data__question="要不要顺便发版？")["data"]["key"]
    t.fake.finish_lead("先交付，T3 的问题请用户回答")
    q = run.wait_event("question.escalated", task="T3")
    c(q, "负责人结束后，协作者的提问转给用户")
    run.act("answer", key=q["data"]["key"], task="T3", answer="beta")
    c("按回答 beta 完成" in run.wait_event("task.succeeded", task="T3")["data"]["summary"], "用户的回答回到 T3 的会话")
    wait(lambda: run.has("run.delivered"), "Run 交付")
    c(True, "处理完所有决定后 Run 交付")
    c(run.wait_event("decision.withdrawn", data__key=unanswered), "负责人交付时，它没等到回答的提问被撤回")
    code, text = run.act_error("answer", key=unanswered, task="", answer="发")
    c(code == 400 and "没有等待中的决定" in text, "撤回的提问不能再回答")
    if ctx.app:
        def questions(s):
            return [e for e in s["open"] + s["later"] if e["run"] == run.id and e["kind"] == "question"]
        ctx.app.until(lambda s: not questions(s), "应用：收件箱里没有这个 Run 撤回的提问", "open inbox", timeout=30)
        c(True, "应用：收件箱里没有这个 Run 撤回的提问")
    ctx.ui(c, run.id, lambda r, s: all(i["kind"] == "session" for i in r["inbox"]) and all(x["status"] == "succeeded" for x in r["tasks"]),
           "没有待处理的事项，T1–T3 都已完成", pages=("board", "task:T1", "audit"))
