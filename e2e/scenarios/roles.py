"""Digital employees by group, and a group exported as role files
(docs/domains/20261031-agents_and_roles.md#role_groups, #role_files)."""

from lib import drawn
from scenarios import scenario


@scenario("roles")
def roles(ctx, c):
    """数字员工按分组列出：默认数字员工各在自己的组；整组改名和换 Agent、导出成文件"""
    t = ctx.team("roles", members=("m1",))
    setup = {r["id"]: r for r in ctx.api("GET", "/api/setup")["roles"]}
    groups = {rid: setup[rid]["group"] for rid in ("pmo", "generalist", "friend", t.member("m1"))}
    c(groups == {"pmo": "产品上线", "generalist": "通用", "friend": "对话", t.member("m1"): ""},
      f"默认数字员工各在自己的组，自己建的不分组（{groups}）")

    made = {}
    for rid, name, lead in (("counsel", "法务顾问", True), ("paralegal", "律师助理", False)):
        made[name] = ctx.api("POST", "/api/setup/roles", {
            "id": rid, "name": name, "desc": "审合同、找风险条款", "group": " 律所 ", "prompt": "逐条读合同，标出风险。",
            "connection": t.member_conn["id"], "lead": lead, "color": "#11845a", "init": "法"})
    counsel = made["法务顾问"]
    c(counsel["group"] == "律所", f"分组名保存时去掉首尾空白（{counsel['group']!r}）")
    code, _ = ctx.api_error("POST", "/api/setup/roles", {"name": "x", "group": "长" * 41})
    c(code == 400, f"超过 40 个字的分组名被拒绝（{code}）")

    # Export: one Markdown file per employee, its front matter and instructions.
    status, body, _ = ctx.api("GET", f"/api/roles/{counsel['id']}/file", raw=True)
    text = body.decode()
    c(status == 200 and text.startswith("---\nid: counsel\n") and 'name: "法务顾问"' in text and 'group: "律所"' in text
      and "lead: true" in text and text.rstrip().endswith("逐条读合同，标出风险。"), "导出的文件带着名字、分组、负责人和指令")

    # A group renamed and put on another agent, as its menu does.
    if ctx.app:
        ctx.app.do("group-rename 律所|律所团队", f"group-bind 律所团队|{t.lead_conn['id']}")
    else:
        ctx.api("POST", "/api/role-groups/rename", {"from": "律所", "to": "律所团队"})
        ctx.api("POST", "/api/role-groups/bind", {"group": "律所团队", "connection": t.lead_conn["id"]})
    now = {r["id"]: r for r in ctx.api("GET", "/api/setup")["roles"]}
    c(all(now[r["id"]]["group"] == "律所团队" and now[r["id"]]["connection"] == t.lead_conn["id"] for r in made.values()),
      "整组改名、整组改用另一个 Agent")
    code, _ = ctx.api_error("POST", "/api/role-groups/bind", {"group": "律所团队", "connection": "no-such-agent"})
    c(code == 400, f"不存在的 Agent 不能整组绑定（{code}）")
    if ctx.app:
        st = ctx.app.until(lambda s: any(g["group"] == "律所团队" for g in s["roleGroups"]), "应用读到改名后的分组", "reload", "open settings:agents", "shot settings-role-groups")
        # Other scenarios add employees meanwhile: each one the app lists sits in its server-side group.
        served = {r["id"]: r["group"] for r in ctx.api("GET", "/api/setup")["roles"]}
        listed = [(g["group"], rid) for g in st["roleGroups"] for rid in g["roles"]]
        c(all(served.get(rid, g) == g for g, rid in listed) and st["roleGroups"][-1]["group"] == ""
          and {rid for g, rid in listed if g == "律所团队"} == {r["id"] for r in made.values()},
          "应用：数字员工按分组列出，每人所在的组和服务一致，未分组排在最后")
        c(drawn(ctx.app.picture("settings-role-groups")), "应用：按分组列出的数字员工画出来了（settings-role-groups.png）")
        # The new-chat page lists them the same way, group by group.
        st = ctx.app.do("close-sheet", "open chats", "shot chats-by-group")
        c("chats" in st["screen"] and drawn(ctx.app.picture("chats-by-group")), "应用：新建对话页按分组列出数字员工（chats-by-group.png）")
