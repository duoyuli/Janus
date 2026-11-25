"""What Janus tells agents follows the app's language: the app tells the service, and each run keeps
the language it was created in."""

import re

from lib import wait
from scenarios import scenario, task

HAN = re.compile(r"[\u4e00-\u9fff]")


@scenario("language", own=True)
def language(ctx, c):
    """注入的语言跟随应用：英文界面的应用连上服务后，服务端记下英文；之后的任务里，负责人说明、协作者约定、看板工具的说明都是英文"""
    with ctx.private("language", settings={"janus.language": "en"}) as ctx:
        wait(lambda: ctx.api("GET", "/api/language")["language"] == "en", "应用把界面语言告诉服务端", 60)
        c(True, "英文界面的应用连上服务后，服务端记下 en")

        t = ctx.team("language", members=("m1",), script={"T1": ["ok"]})
        m1 = t.member("m1")
        run = t.run("Write the notes")
        lead = run.lead()
        brief = next(e["data"]["prompt"] for e in run.events()
                     if e["type"] == "session.prompt" and e.get("to") == "lead" and e["data"].get("channel") == "system")
        c(brief.startswith("You are the lead of the Janus team") and "janus.orchestrate:" not in brief,
          "负责人说明是英文，不列看板工具")
        status, body = lead.post({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})
        tools = {x["name"]: x["description"] for x in body["result"]["tools"]}
        c(status == 200 and tools and not any(HAN.search(d) for d in tools.values()),
          f"看板工具的说明是英文：{sorted(tools)}")
        status, body = lead.post({"jsonrpc": "2.0", "id": 2, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}})
        c(status == 200 and not (body["result"].get("instructions") or ""), "看板服务器不返回 instructions，用法只在工具说明里")

        run.orchestrate(task("T1", m1))
        run.wait_event("task.succeeded", task="T1")
        rules = next(e["data"]["prompt"] for e in run.events()
                     if e["type"] == "session.prompt" and e.get("task") == "T1" and e["data"].get("channel") == "system")
        c(rules.startswith("You are a collaborator on the Janus team") and "janus.board_post" not in rules,
          "协作者约定是英文，不介绍看板工具")
        first = run.has("attempt.started", task="T1")[0]["data"]["prompt"]
        c("Your subtask T1:" in first and "Acceptance criteria:" in first, "协作者的任务提示是英文")
