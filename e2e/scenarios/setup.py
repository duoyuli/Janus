"""First-run setup: what a person does in Settings before the first goal."""

import json

import os

from lib import ADAPTER_PACKAGES, ROOT, STAND_IN_VERSION, Fake, FAKE_AGENT, git_repo, wait
from scenarios import scenario, task, Team

SECRET = "secret-e2e-value-7f3a"


@scenario("setup", first=True)
def setup(ctx, c):
    """首次配置：Agent 检查、默认数字员工、负责人规则、资源库和两个路由、工作区，再用默认团队跑通一个目标"""
    meta = ctx.api("GET", "/api/meta")
    c(meta["dataDir"] == ctx.home, f"服务使用临时 Janus home（{meta['dataDir']}）")
    c(meta.get("version") and isinstance(meta.get("dataVersion"), int), f"meta 带版本 {meta.get('version')} 和数据版本 {meta.get('dataVersion')}")
    c({"codex", "claude", "custom"} <= {k["id"] for k in meta["kinds"]}, "Agent 类型包括 Codex、Claude Code 和自定义命令")
    adapters = {a["id"]: a for a in ctx.api("GET", "/api/adapters")["adapters"]}
    c(all(adapters[i]["current"] == STAND_IN_VERSION for i in ADAPTER_PACKAGES), "Codex 和 Claude Code 的适配器是替身版本，不联网安装")

    setup_ = ctx.api("GET", "/api/setup")
    seeded = {r["id"] for r in setup_["roles"]}
    c({"coordinator", "generalist", "researcher", "product-manager", "implementer", "reviewer"} <= seeded, "首次启动写入 6 个默认数字员工，包括通用数字员工")

    # Agents of every built-in kind, checked the way Settings → Digital Employees does.
    fake = Fake(ctx.path("setup", "fake"))
    env = f"FAKE_DIR={fake.dir}"
    conns = [
        {"id": "setup-codex", "kind": "codex", "name": "Codex 替身", "auth": "apikey", "apiKey": SECRET, "envText": env},
        {"id": "setup-claude", "kind": "claude", "name": "Claude 替身", "auth": "apikey", "apiKey": SECRET, "envText": env},
        {"id": "setup-fake", "kind": "custom", "name": "脚本 Agent", "command": f"{ctx.python} {FAKE_AGENT}", "envText": env},
    ]
    for body in conns:
        ctx.api("POST", "/api/setup/connections", body)
        ctx.api("POST", f"/api/connections/{body['id']}/test")
    def checked():
        cs = {x["id"]: x for x in ctx.api("GET", "/api/setup")["connections"]}
        phases = {i["id"]: cs[i["id"]].get("phase") for i in conns}
        return cs if all(p in ("ok", "error") for p in phases.values()) else None
    cs = wait(checked, "三个 Agent 的配置检查结束", 90)
    for body in conns:
        x = cs[body["id"]]
        c(x.get("phase") == "ok", f"配置检查通过：{body['name']}（{x.get('phase')} {x.get('error') or ''}）")
        controls = [o["id"] for o in (x.get("options") or {}).get("controls", [])]
        c("mode" in controls, f"{body['name']} 的会话控制项读进来了：{controls}")
    c(SECRET not in json.dumps(ctx.api("GET", "/api/setup")), "设置接口不返回凭据的值")
    c("unknown ok" in fake.calls(), "配置检查发出了试运行消息")

    # Only Codex and Claude Code lead, in a release build too.
    ctx.api("POST", "/api/setup/roles", {"id": "setup-custom-lead", "name": "自定义负责人", "desc": "x", "connection": "setup-fake", "lead": True})
    repo = git_repo(ctx.path("setup", "repo"))
    code, text = ctx.api_error("POST", "/api/setup/workspaces", {
        "id": "setup-bad", "name": "bad", "path": repo, "lead": "setup-custom-lead", "members": [{"role": "setup-custom-lead"}, {"role": "implementer"}]})
    c(code == 400, f"自定义命令的 Agent 不能担任负责人（{code} {text.strip()[:80]}）")
    code, _ = ctx.api_error("POST", "/api/setup/workspaces", {"id": "setup-bad", "name": "bad", "path": repo + "/missing", "lead": "coordinator", "members": [{"role": "coordinator"}]})
    c(code == 400, "不存在的工作目录被拒绝")

    # The library: a Skill and an MCP server with a secret value, chosen for a digital employee.
    skill = ctx.api("POST", "/api/library/skills", {"name": "e2e-skill", "description": "端到端测试用",
                                                    "content": "---\nname: e2e-skill\ndescription: 端到端测试用\n---\n先读 README。\n"})
    mcp = ctx.api("POST", "/api/library/mcp", {"name": "e2e-mcp", "description": "", "transport": "stdio", "command": "/usr/bin/true",
                                               "vars": [{"name": "E2E_TOKEN", "value": SECRET}]})
    lib = ctx.api("GET", "/api/library")
    item = next(i for i in lib["items"] if i["id"] == mcp["id"])
    c(item["vars"] == [{"name": "E2E_TOKEN", "set": True}] and SECRET not in json.dumps(lib), "资源库只说 MCP 的值已填写，不返回值")
    c(ctx.api("GET", f"/api/library/items/{skill['id']}")["content"].startswith("---"), "Skill 保存了 SKILL.md 全文")
    c((skill["reach"], mcp["reach"]) == ("everyone", "picked"), "新 Skill 默认给所有数字员工，新 MCP 默认收口")
    # A command server opened to everyone: served by each run's MCP router, not handed to sessions.
    starts = os.path.join(ctx.path("setup"), "route-starts.log")
    routed = ctx.api("POST", "/api/library/mcp", {"name": "e2e-route", "description": "路由测试", "transport": "stdio",
                                                  "command": f"{ctx.python} {os.path.join(ROOT, 'backend', 'testdata', 'fake_mcp.py')}",
                                                  "vars": [{"name": "FAKE_MCP_LOG", "value": starts}]})
    routed = ctx.api("POST", f"/api/library/items/{routed['id']}/reach", {"reach": "everyone"})
    c(routed["reach"] == "everyone", "MCP 服务器可以放开给所有数字员工")
    code, _ = ctx.api_error("POST", f"/api/library/items/{routed['id']}/reach", {"reach": "nobody"})
    c(code == 400, "给谁用只接受 everyone 和 picked")

    # The default team, bound to the agents: PMO leads on Codex, the engineer runs the script.
    roles = {r["id"]: r for r in ctx.api("GET", "/api/setup")["roles"]}
    ctx.api("POST", "/api/setup/roles", dict(roles["coordinator"], connection="setup-codex", lead=True))
    ctx.api("POST", "/api/setup/roles", dict(roles["implementer"], connection="setup-fake", resources=[mcp["id"], skill["id"]]))
    ctx.api("POST", "/api/setup/roles", dict(roles["reviewer"], connection="setup-claude"))
    used = next(i for i in ctx.api("GET", "/api/library")["items"] if i["id"] == mcp["id"])
    c([r["id"] for r in used["roles"]] == ["implementer"], "资源库标出使用 MCP 的数字员工")
    # The same choice from the library's side: given to the reviewer too, then back to the engineer only.
    got = ctx.api("POST", f"/api/library/items/{mcp['id']}/roles", {"roles": ["implementer", "reviewer"]})
    c([r["id"] for r in got["roles"]] == ["implementer", "reviewer"], "资源库里可以把收口的 MCP 勾给数字员工")
    code, _ = ctx.api_error("POST", f"/api/library/items/{mcp['id']}/roles", {"roles": ["nobody"]})
    c(code == 400, "勾给不存在的数字员工被拒绝")
    roles = {r["id"]: r for r in ctx.api("GET", "/api/setup")["roles"]}
    edit = {k: v for k, v in roles["reviewer"].items() if k != "resources"}
    ctx.api("POST", "/api/setup/roles", dict(edit, desc=roles["reviewer"]["desc"]))
    roles = {r["id"]: r for r in ctx.api("GET", "/api/setup")["roles"]}
    c(mcp["id"] in roles["reviewer"]["resources"], "保存数字员工时不带资源，保留资源库里给它的选择")
    ctx.api("POST", f"/api/library/items/{mcp['id']}/roles", {"roles": ["implementer"]})
    code, _ = ctx.api_error("DELETE", f"/api/library/items/{mcp['id']}")
    c(code == 400, "被数字员工使用的资源不能删除")
    ws = ctx.api("POST", "/api/setup/workspaces", {"id": "setup", "name": "默认团队", "path": repo, "lead": "coordinator", "brief": "共享资料：用 Python 3。",
                                                   "members": [{"role": "coordinator"}, {"role": "implementer"}, {"role": "reviewer"}]})
    c(ws["path"] == repo, "工作区保存规范化的绝对路径")
    insp = ctx.api("GET", f"/api/inspect?path={repo}")
    c(insp.get("git") is True, f"检查目录识别出 Git 仓库：{insp}")
    code, _ = ctx.api_error("DELETE", "/api/setup/roles/implementer")
    c(code == 400, "被工作区使用的数字员工不能删除")

    team = Team(ctx, "setup", fake, repo, ws, {"implementer": "implementer", "reviewer": "reviewer"}, cs["setup-codex"], cs["setup-fake"])
    fake.script({"T1": ["write:hello.py"], "V1": ["approve"]})
    run = team.run("写一个 hello.py")
    fake.wait_call("goal 写一个 hello.py")
    c(True, "负责人（Codex 替身）收到 /goal")
    run.orchestrate(task("T1", "implementer", owns=["hello.py"]), task("V1", "reviewer", verifies="T1"))
    board = run.read_board(lambda b: "[完成] V1" in b)
    c("[完成] T1" in board and "[完成] V1" in board, "研发完成，质量工程师（Claude 替身）独立验证完成")
    sessions = [line for line in fake.calls().splitlines() if line.startswith("mcp ") and f"/mcp/{run.id}/" in line and '/T1"' in line]
    c(sessions and SECRET in sessions[-1] and "e2e-mcp" in sessions[-1], "角色的 MCP 连同值进入 T1 的 session/new")
    servers = json.loads(sessions[-1][len("mcp "):])
    router = next((x for x in servers if x["name"] == "janus-tools"), None)
    c(router and not any(x["name"] == "e2e-route" for x in servers), "给所有人的命令服务器不直接交给会话，T1 拿到 janus-tools")
    path = "/" + router["url"].split("/", 3)[3]
    def rpc(method, params, at=path):
        return ctx.api("POST", at, {"jsonrpc": "2.0", "id": 1, "method": method, "params": params})["result"]
    names = [t["name"] for t in rpc("tools/list", {})["tools"]]
    c("e2e-route__cwd" in names, f"MCP 路由列出服务器的工具：{names}")
    rpc("tools/list", {})
    c(open(starts).read().count("start") == 1, "同一个 Run 的会话共用一个服务器进程")
    got = rpc("tools/call", {"name": "e2e-route__cwd", "arguments": {}})
    c(got["content"][0]["text"] == run.cwd(), "路由背后的服务器在 Run 的工作目录（worktree）里启动")
    lead_path = path.rsplit("/", 2)[0] + "/lead/tools"
    code = ctx.api_error("POST", lead_path, {"jsonrpc": "2.0", "id": 1, "method": "tools/list"})[0]
    c(code in (401, 403, 404), f"协作者的令牌不能冒充负责人使用路由（{code}）")
    calls_ = run.has("mcp.routed")
    c(calls_ and calls_[0]["data"]["tool"] == "e2e-route__cwd" and "arguments" not in calls_[0]["data"], "每次路由调用记一条 mcp.routed，不记参数")
    detail = json.dumps(run.detail(), ensure_ascii=False)
    c(SECRET not in detail and SECRET not in ctx.api("GET", f"/api/runs/{run.id}/export", raw=True)[1].decode(), "Run 记录和导出中没有 MCP 的值")
    c("共享资料：用 Python 3。" in detail, "工作区共享资料进入负责人说明")
    run.finish("hello.py 写好了")
    c(run.status() == "delivered", "负责人结束后 Run 交付")

    if ctx.app:
        st = ctx.app.until(lambda s: {x["id"]: x["phase"] for x in s["connections"]}.get("setup-codex") == "ok"
                           and "setup" in [w["id"] for w in s["workspaces"]], "应用读到 Agent、数字员工和工作区", "reload", "open home:setup")
        c(True, "应用：设置里的 Agent 状态与服务一致（检查通过）")
        c({"coordinator", "implementer"} <= {r["id"] for r in st["roles"]}, "应用：读到默认数字员工")
        for tab in ("general", "agents", "library", "usage", "permissions", "updates"):
            ctx.app.do(f"open settings:{tab}", f"shot settings-{tab}")
            c(_drawn(ctx, f"settings-{tab}"), f"应用：设置 → {tab} 画出来了")
        ctx.app.do("close-sheet", "role implementer", "shot role-editor", "close-sheet", "workspace setup", "shot workspace-editor", "close-sheet")
        c(_drawn(ctx, "role-editor"), "应用：数字员工编辑器画出来了")
        c(_drawn(ctx, "workspace-editor"), "应用：工作区编辑器画出来了")
        ctx.ui(c, run.id, lambda r, s: r["status"] == "delivered" and {t["id"]: t["status"] for t in r["tasks"]} == {"T1": "succeeded", "V1": "succeeded"}
               and next(t for t in r["tasks"] if t["id"] == "V1")["verdict"] == "approved",
               "Run 已交付，T1、V1 已完成，V1 判定通过", pages=("board", "team", "audit", "task:T1", "task:V1", "agent:lead"))


def _drawn(ctx, name):
    from lib import drawn
    return drawn(ctx.app.picture(name))
