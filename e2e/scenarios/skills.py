"""Library skills reach agents only through janus-skills: the catalog a session is pointed at, the
reads it leads to, and what the library shows of them (docs/domains/20261116-resource_library.md#skill_router)."""

import re

from scenarios import read_file, scenario, task
from lib import drawn


@scenario("skills")
def skills(ctx, c):
    """Skill 路由：会话说明指向 janus-skills 的目录，Agent 从目录找到 Skill 再读；读取记为 skill.used，资源库显示用过几次和描述的改进提示"""
    good = ctx.api("POST", "/api/library/skills", {
        "name": "e2e-minutes", "description": "把会议记录整理成决定、待办和负责人。在用户要整理会议记录或会议纪要时使用",
        "content": "按决定、待办、负责人三段写。\n"})
    vague = ctx.api("POST", "/api/library/skills", {"name": "e2e-docs", "description": "处理文档", "content": "看情况处理。\n"})
    t = ctx.team("skills", members=("m1",), script={"T1": ["skill:e2e-minutes"]})
    m1 = t.member("m1")
    run = t.run("把周会记录整理一下")
    run.orchestrate(task("T1", m1))
    run.wait_event("task.succeeded", task="T1")
    t.fake.wait_call("skill e2e-minutes read")
    c(True, "协作者照系统层的指路读 janus-skills 的目录，在目录里找到 e2e-minutes，再读它的 SKILL.md")

    pointers = {e.get("to"): e["data"]["prompt"] for e in run.events() if e["type"] == "session.prompt" and e["data"].get("channel") == "skills"}
    c({"lead", m1} <= set(pointers), f"负责人和协作者的系统层都写明 janus-skills 的目录在哪里：{sorted(pointers)}")
    md = read_file(re.search(r"read (\S+/janus-skills/SKILL\.md)", pointers[m1]).group(1))
    desc = next(line for line in md.splitlines() if line.startswith("description: "))
    c("开始一项任务时先读它" in desc and "e2e-minutes（把会议记录整理成决定" in desc,
      f"janus-skills 的描述写明什么时候读它，带上各个 Skill 的描述片段：{desc[:160]}")
    c("- e2e-minutes：把会议记录整理成决定、待办和负责人。在用户要整理会议记录或会议纪要时使用" in md, "目录里每个 Skill 一行，写着完整描述")

    used = run.has("skill.used", task="T1")
    c(len(used) == 1 and used[0]["actor"] == m1 and used[0]["data"]["item"] == good["id"],
      f"读 Skill 的 SKILL.md 记一条 skill.used，读目录不算：{[u['data'] for u in used]}")
    lib = ctx.api("GET", "/api/library")
    items = {i["id"]: i for i in lib["items"]}
    c(items[good["id"]]["uses"]["sessions"] == 1 and items[good["id"]]["uses"]["lastAt"], f"资源库记下 e2e-minutes 被 1 个会话读过：{items[good['id']]['uses']}")
    c(items[good["id"]]["hints"] == [], "写清了做什么和什么时候用的 Skill 没有提示")
    hints = items[vague["id"]]["hints"]
    c(items[vague["id"]]["uses"]["sessions"] == 0 and hints == [{"kind": "short"}], f"描述太短的 Skill 带着改进提示：{hints}")
    c(lib["skillRouter"]["catalog"] == "list", "Skill 不多时，目录直接列出每个 Skill")
    run.finish("整理好了")

    if ctx.app:
        names = ("e2e-minutes", "e2e-docs")
        shown = lambda s: {i["name"]: i for i in s.get("skills", {}).get("items", []) if i["name"] in names}  # noqa: E731
        st = ctx.app.until(lambda s: shown(s).get("e2e-minutes", {}).get("sessions") == 1,
                           "应用读到 Skill 的使用次数", "reload", "open settings:library:skill")
        server = {i["name"]: i for i in ctx.api("GET", "/api/library")["items"] if i["name"] in names}
        app = shown(st)
        for name in names:
            c(app[name]["sessions"] == server[name]["uses"]["sessions"], f"应用：{name} 被几个会话读过和服务端一致（{app[name]['sessions']}）")
            c(app[name]["hints"] == [h["kind"] for h in server[name]["hints"]] and all(app[name]["hintText"]),
              f"应用：{name} 的改进提示和服务端一致，每条都写成了界面语言的文字（{app[name]['hintText']}）")
        c("1" in app["e2e-minutes"]["usage"] and app["e2e-minutes"]["usage"] != app["e2e-docs"]["usage"],
          f"应用：e2e-minutes 显示被几个会话读过、最近一次在什么时候（{app['e2e-minutes']['usage']}）")
        c(app["e2e-docs"]["warning"] and not app["e2e-minutes"]["warning"], f"应用：有提示的 e2e-docs 在行尾标出描述可以改进（{app['e2e-docs']['warning']}）")
        order = [i["name"] for i in st["skills"]["items"]]
        c(order.index("e2e-minutes") < order.index("e2e-docs"), "应用：Skill 路由说明按目录的顺序排，读过的在前（e2e-docs 按名字本应在前）")
        c(st["skills"]["searched"] == (ctx.api("GET", "/api/library")["skillRouter"]["catalog"] == "search"), "应用：说明里的目录方式和服务端一致")
        ctx.app.do("shot library-skill-router")
        c(drawn(ctx.app.picture("library-skill-router")), "应用：资源库的 Skill 页和路由说明画出来了（library-skill-router.png）")
        created = {g["key"]: g["items"] for g in st["skills"]["groups"]}.get("created", [])
        c(set(names) <= set(created), f"应用：在 Janus 中新建的两个 Skill 挂在「在 Janus 中新建」一组下（{st['skills']['groups']}）")
        ctx.app.do("size 1280x1600", "shot library-skill-groups")
        c(drawn(ctx.app.picture("library-skill-groups")), "应用：已添加的 Skill 按来源分组，挂在组标题下画出来了（library-skill-groups.png）")
