"""Digital employees by group, as role files in and out, and 招聘顾问 designing a team in a chat
(docs/domains/20261031-agents_and_roles.md#role_groups, #role_files)."""

import os

from lib import drawn, wait
from scenarios import scenario

COUNSEL = """---
name: 法务顾问
desc: 审合同、找风险条款；需要逐条审阅合同时交给它
group: 律所
color: "#11845a"
init: 法
lead: true
---

逐条读合同，标出风险。
"""

PARALEGAL = """---
id: paralegal
name: 律师助理
desc: 整理案卷和证据目录
group: 律所
---

整理材料，列出缺什么。
"""

# A Claude Code subagent as it is shared: a long description with examples, its own tools and model.
SUBAGENT = """---
name: code-reviewer
description: Reviews diffs for bugs and risky changes.\\n\\n<example>Context: a pull request</example>
tools: Read, Grep
model: sonnet
color: blue
---

You review code and report what could break.
"""


def write(root, rel, text):
    path = os.path.join(root, rel)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        f.write(text)


@scenario("roles")
def roles(ctx, c):
    """数字员工按分组列出：默认数字员工各在自己的组；从文件夹导入（带 subagent 文件）、整组改名和换 Agent、导出成文件；招聘顾问在对话里写出的团队从对话里导入"""
    t = ctx.team("roles", members=("m1",))
    setup = {r["id"]: r for r in ctx.api("GET", "/api/setup")["roles"]}
    groups = {rid: setup[rid]["group"] for rid in ("pmo", "generalist", "friend", "recruiter", t.member("m1"))}
    c(groups == {"pmo": "产品上线", "generalist": "通用", "friend": "对话", "recruiter": "对话", t.member("m1"): ""},
      f"默认数字员工各在自己的组，自己建的不分组（{groups}）")

    # A folder of role files: two by hand under legal/, a Claude Code subagent, a README.
    src = ctx.path("roles", "files")
    write(src, "legal/counsel.md", COUNSEL)
    write(src, "legal/paralegal.md", PARALEGAL)
    write(src, "subagents/code-reviewer.md", SUBAGENT)
    write(src, "README.md", "# 这些是数字员工\n")
    scan = ctx.api("POST", "/api/roles/import/scan", {"path": src})
    found = {f["file"]: f for f in scan["roles"]}
    c(sorted(found) == ["legal/counsel.md", "legal/paralegal.md", "subagents/code-reviewer.md"], f"扫描找到三个数字员工文件，README 不算（{sorted(found)}）")
    reviewer = found["subagents/code-reviewer.md"]
    c(reviewer["desc"] == "Reviews diffs for bugs and risky changes." and reviewer["group"] == "subagents" and reviewer["color"] == "",
      f"subagent 文件的职责取描述第一段、去掉示例，没写分组时取文件夹名（{reviewer['desc']} / {reviewer['group']}）")
    code, _ = ctx.api_error("POST", "/api/roles/import", {"path": src, "files": ["../elsewhere.md"], "connection": ""})
    c(code == 400, f"只能导入扫描列出的文件（{code}）")

    files = ["legal/counsel.md", "legal/paralegal.md", "subagents/code-reviewer.md"]
    if ctx.app:
        st = ctx.app.do(f"roles-import {src}", "shot roles-import")
        im = st["roleImport"]
        c([(f["file"], f["name"], f["group"]) for f in im["found"]] == [(f["file"], f["name"], f["group"]) for f in scan["roles"]]
          and im["picked"] == files, "应用：导入表单列出的文件、名字和分组与服务扫描的一致，全部默认勾选")
        c(drawn(ctx.app.picture("roles-import")), "应用：导入表单画出来了（roles-import.png）")
        st = ctx.app.do("roles-import-toggle subagents/code-reviewer.md", f"roles-import-agent {t.member_conn['id']}", "roles-import-go")
        c(st["roleImport"] is None, "应用：导入后表单关闭")
    else:
        ctx.api("POST", "/api/roles/import", {"path": src, "files": files[:2], "connection": t.member_conn["id"]})
    after = {r["id"]: r for r in ctx.api("GET", "/api/setup")["roles"]}
    made = {r["name"]: r for r in after.values() if r["id"] not in setup}
    c(sorted(made) == ["律师助理", "法务顾问"], f"只导入勾选的两位（{sorted(made)}）")
    counsel, paralegal = made["法务顾问"], made["律师助理"]
    c(counsel["lead"] and counsel["group"] == "律所" and counsel["color"] == "#11845a" and counsel["prompt"] == "逐条读合同，标出风险。",
      "导入的数字员工带着文件里的负责人、分组、颜色和指令")
    c(paralegal["id"] == "paralegal" and all(r["connection"] == t.member_conn["id"] for r in made.values()), "文件里的 id 照用，整批绑定选的 Agent")

    # Importing again never replaces anyone: the names are taken, so nothing is ticked; forced, it gets a new id.
    again = ctx.api("POST", "/api/roles/import/scan", {"path": os.path.join(src, "legal")})
    c(all(f["existing"] for f in again["roles"]), "再扫描时同名的标为已有")
    second = ctx.api("POST", "/api/roles/import", {"path": os.path.join(src, "legal"), "files": ["paralegal.md"], "connection": ""})
    c(second[0]["id"] == "paralegal-2" and ctx.api("GET", "/api/setup")["roles"] and
      {r["id"]: r for r in ctx.api("GET", "/api/setup")["roles"]}["paralegal"]["connection"] == t.member_conn["id"],
      "已有的 id 不被覆盖，新导入的另取 id")
    ctx.api("DELETE", f"/api/setup/roles/{second[0]['id']}")

    # Export writes what import reads back.
    status, body, _ = ctx.api("GET", f"/api/roles/{counsel['id']}/file", raw=True)
    out = ctx.path("roles", "export", "律所")
    write(out, f"{counsel['id']}.md", body.decode())
    back = ctx.api("POST", "/api/roles/import/scan", {"path": out})["roles"][0]
    c(status == 200 and (back["name"], back["desc"], back["group"], back["lead"], back["prompt"]) ==
      (counsel["name"], counsel["desc"], counsel["group"], counsel["lead"], counsel["prompt"]), "导出的文件再扫描，内容和数字员工一致")

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

    # 招聘顾问: a chat whose agent writes role files into its folder, which the chat offers to import.
    ctx.api("POST", "/api/setup/roles", dict({k: v for k, v in setup["recruiter"].items() if k != "seedUpdates"}, connection=t.member_conn["id"]))
    write(t.fake.dir, "team/电商运营/shop-lead.md", COUNSEL.replace("法务顾问", "店铺负责人").replace("律所", "电商运营"))
    write(t.fake.dir, "team/电商运营/copywriter.md", PARALEGAL.replace("律师助理", "文案").replace("律所", "电商运营").replace("paralegal", "copywriter"))
    if ctx.app:
        # Adding someone by hand offers 招聘顾问 instead; editing someone does not.
        st = ctx.app.do("role new", "shot role-editor-new")
        c(st["editingRole"]["offersRecruiter"] is True and drawn(ctx.app.picture("role-editor-new")),
          "应用：新建数字员工的表单里有去找招聘顾问的入口（role-editor-new.png）")
        st = ctx.app.do("close-sheet", f"role {counsel['id']}")
        c(st["editingRole"]["offersRecruiter"] is False, "应用：编辑已有的数字员工时不提示招聘顾问")
        ctx.app.do("close-sheet")
        # A chat starts only on an agent that passed its check, as in Settings.
        ctx.api("POST", f"/api/connections/{t.member_conn['id']}/test")
        wait(lambda: {x["id"]: x["phase"] for x in ctx.api("GET", "/api/setup")["connections"]}.get(t.member_conn["id"]) == "ok", "Agent 检查通过", 90)
        st = ctx.app.until(lambda s: (s.get("run") or {}).get("chat"), "应用打开和招聘顾问的对话", "reload", "recruiter-chat")
        chat = st["run"]["id"]
    else:
        chat = ctx.api("POST", "/api/chats", {"role": "recruiter"})["id"]
    c([m["role_id"] for m in ctx.api("GET", f"/api/runs/{chat}")["run"]["config"]["team"]] == ["recruiter"], "对话的数字员工是招聘顾问")
    wait(lambda: [e for e in ctx.api("GET", f"/api/runs/{chat}")["events"] if e["type"] == "agent.status" and e["data"].get("turn") == "idle"],
         "招聘顾问的会话打开", 60)
    ctx.api("POST", f"/api/runs/{chat}/actions", {"type": "message", "agent": "lead", "text": "开一家网店 [team]"})
    cwd = ctx.api("GET", f"/api/runs/{chat}")["run"]["cwd"]
    wait(lambda: len(ctx.api("POST", "/api/roles/import/scan", {"path": cwd})["roles"]) == 2, "招聘顾问把两位数字员工写进对话目录", 60)
    if ctx.app:
        ctx.ui(c, chat, lambda r, s: r.get("designedRoles") == 2, "对话页提示目录里有 2 位数字员工的文件，可以导入", pages=("density detail",))
        st = ctx.app.do(f"roles-import {cwd}", f"roles-import-agent {t.member_conn['id']}")
        c(sorted(f["file"] for f in st["roleImport"]["found"]) == ["电商运营/copywriter.md", "电商运营/shop-lead.md"], "应用：从对话目录打开的导入表单列出这两位")
        ctx.app.do("roles-import-go")
    else:
        ctx.api("POST", "/api/roles/import", {"path": cwd, "files": ["电商运营/copywriter.md", "电商运营/shop-lead.md"], "connection": t.member_conn["id"]})
    shop = [r for r in ctx.api("GET", "/api/setup")["roles"] if r["group"] == "电商运营"]
    c(sorted(r["name"] for r in shop) == ["店铺负责人", "文案"], "招聘顾问设计的团队导入后成为「电商运营」组")
