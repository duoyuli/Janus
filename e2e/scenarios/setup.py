"""First-run setup: what a person does in Settings before the first goal."""

import atexit
import base64
import json
import os
import subprocess

from lib import ADAPTER_PACKAGES, ROOT, STAND_IN_VERSION, Fake, git_repo, wait
from scenarios import scenario, task, Team

SECRET = "secret-e2e-value-7f3a"


@scenario("setup", first=True)
def setup(ctx, c):
    """首次配置：Agent 检查、默认数字员工、负责人规则、资源库和两个路由、工作区，再用默认团队跑通一个目标"""
    meta = ctx.api("GET", "/api/meta")
    c(meta["home"] == ctx.home, f"服务使用临时 Janus home（{meta['home']}）")
    c(meta.get("version") and isinstance(meta.get("dataVersion"), int), f"meta 带版本 {meta.get('version')} 和数据版本 {meta.get('dataVersion')}")
    c({k["id"] for k in meta["kinds"]} == {"codex", "claude"}, "Agent 类型只有 Codex 和 Claude Code")
    code, _ = ctx.api_error("POST", "/api/setup/connections", {"kind": "custom", "name": "x", "command": "true"})
    c(code == 400, f"不能再添加自定义命令的 Agent（{code}）")
    adapters = {a["id"]: a for a in ctx.api("GET", "/api/adapters")["adapters"]}
    c(all(adapters[i]["current"] == STAND_IN_VERSION for i in ADAPTER_PACKAGES), "Codex 和 Claude Code 的适配器是替身版本，不联网安装")

    setup_ = ctx.api("GET", "/api/setup")
    seeded = {r["id"] for r in setup_["roles"]}
    c({"pmo", "generalist", "marketing", "product-manager", "developer", "qa", "friend"} <= seeded,
      "首次启动写入 7 个默认数字员工，包括通用数字员工和「老友」")

    # Agents of every built-in kind, checked the way Settings → Digital Employees does.
    fake = Fake(ctx.path("setup", "fake"))
    env = f"FAKE_DIR={fake.dir}"
    claude_env = f"{env}\nexport ANTHROPIC_AUTH_TOKEN={SECRET}\nexport ANTHROPIC_BASE_URL=https://gateway.example.com\nexport ANTHROPIC_MODEL=deepseek-v4.1-flash"
    auths = next(k["auths"] for k in meta["kinds"] if k["id"] == "claude")
    custom = auths[-1]
    template = custom.get("template", "")
    c([a["id"] for a in auths] == ["import", "oauth", "apikey"] and custom["label"] == "自定义 API" and custom.get("env")
      and not custom["fields"] and "export ANTHROPIC_AUTH_TOKEN=" in template and "export ANTHROPIC_BASE_URL=" in template,
      "Claude 的自定义 API 用环境变量设置，没有单独的 API Key 字段，并带模板")
    conns = [
        {"id": "setup-codex", "kind": "codex", "name": "Codex 替身", "auth": "apikey", "apiKey": SECRET, "envText": env},
        {"id": "setup-claude", "kind": "claude", "name": "Claude 替身", "auth": "apikey", "envText": claude_env},
    ]
    for body in conns:
        saved = ctx.api("POST", "/api/setup/connections", body)
        c(SECRET not in json.dumps(saved), f"保存 {body['name']} 的响应隐藏凭据")
        ctx.api("POST", f"/api/connections/{body['id']}/test")
    def checked():
        cs = {x["id"]: x for x in ctx.api("GET", "/api/setup")["connections"]}
        phases = {i["id"]: cs[i["id"]].get("phase") for i in conns}
        return cs if all(p in ("ok", "error") for p in phases.values()) else None
    cs = wait(checked, "两个 Agent 的配置检查结束", 90)
    for body in conns:
        x = cs[body["id"]]
        c(x.get("phase") == "ok", f"配置检查通过：{body['name']}（{x.get('phase')} {x.get('error') or ''}）")
        controls = [o["id"] for o in (x.get("options") or {}).get("controls", [])]
        c("mode" in controls, f"{body['name']} 的会话控制项读进来了：{controls}")
    c(SECRET not in json.dumps(ctx.api("GET", "/api/setup")), "设置接口不返回凭据的值")
    c("unknown ok" in fake.calls(), "配置检查发出了试运行消息")

    # A lead needs an agent.
    ctx.api("POST", "/api/setup/roles", {"id": "setup-unbound-lead", "name": "没绑定的负责人", "desc": "x", "connection": "", "lead": True})
    repo = git_repo(ctx.path("setup", "repo"))
    code, text = ctx.api_error("POST", "/api/setup/workspaces", {
        "id": "setup-bad", "name": "bad", "path": repo, "lead": "setup-unbound-lead", "members": [{"role": "setup-unbound-lead"}, {"role": "developer"}]})
    c(code == 400, f"没有绑定 Agent 的数字员工不能担任负责人（{code} {text.strip()[:80]}）")
    code, _ = ctx.api_error("POST", "/api/setup/workspaces", {"id": "setup-bad", "name": "bad", "path": repo + "/missing", "lead": "pmo", "members": [{"role": "pmo"}]})
    c(code == 400, "不存在的工作目录被拒绝")

    # The library: a Skill and an MCP server with a secret value, both for every digital employee.
    skill = ctx.api("POST", "/api/library/skills", {"name": "e2e-skill", "description": "端到端测试用",
                                                    "content": "---\nname: e2e-skill\ndescription: 端到端测试用\n---\n先读 README。\n"})
    mcp = ctx.api("POST", "/api/library/mcp", {"name": "e2e-mcp", "description": "", "transport": "stdio", "command": "/usr/bin/true",
                                               "vars": [{"name": "E2E_TOKEN", "value": SECRET}]})
    lib = ctx.api("GET", "/api/library")
    item = next(i for i in lib["items"] if i["id"] == mcp["id"])
    c(item["vars"] == [{"name": "E2E_TOKEN", "set": True}] and SECRET not in json.dumps(lib), "资源库只说 MCP 的值已填写，不返回值")
    env_url = "/api/connections/setup-claude/env"
    c(ctx.api("GET", env_url)["env"]["ANTHROPIC_AUTH_TOKEN"] == SECRET, "Agent 编辑器从本机读到保存的令牌")
    for headers, why in (({"Origin": "https://evil.example"}, "网页发来的请求"), ({"Host": "evil.example"}, "改绑到本机的域名")):
        code, text = ctx.api_error("GET", env_url, headers=headers)
        c(code == 403 and SECRET not in text, f"{why}读不到 Agent 的环境变量（{code}）")
    values = f"/api/library/mcp/{mcp['id']}/values"
    c(ctx.api("GET", values) == {"values": {"E2E_TOKEN": SECRET}}, "MCP 编辑器从本机读到保存的值")
    for headers, why in (({"Origin": "https://evil.example"}, "网页发来的请求"), ({"Host": "evil.example"}, "改绑到本机的域名"),
                         ({"Sec-Fetch-Site": "same-origin"}, "浏览器发来的请求")):
        code, text = ctx.api_error("GET", values, headers=headers)
        c(code == 403 and SECRET not in text, f"{why}读不到 MCP 的值（{code}）")
    c(ctx.api("GET", f"/api/library/items/{skill['id']}")["content"].startswith("---"), "Skill 保存了 SKILL.md 全文")
    c("reach" not in skill and "reach" not in mcp, "资源没有「给谁用」：每项都给所有数字员工")
    # A command server: served by each run's MCP router, not handed to sessions.
    starts = os.path.join(ctx.path("setup"), "route-starts.log")
    routed = ctx.api("POST", "/api/library/mcp", {"name": "e2e-route", "description": "路由测试", "transport": "stdio",
                                                  "command": f"{ctx.python} {os.path.join(ROOT, 'backend', 'testdata', 'fake_mcp.py')}",
                                                  "vars": [{"name": "FAKE_MCP_LOG", "value": starts}]})
    for path, body in (("reach", {"reach": "picked"}), ("roles", {"roles": ["developer"]})):
        code, _ = ctx.api_error("POST", f"/api/library/items/{routed['id']}/{path}", body)
        c(code in (404, 405), f"不能再把资源收口或勾给个别数字员工（{path}：{code}）")
    # A Streamable HTTP server with a credential header: Janus reaches it, the agent never sees it.
    http_log = os.path.join(ctx.path("setup"), "http-mcp.log")
    http_server = subprocess.Popen([ctx.python, os.path.join(ROOT, "backend", "testdata", "fake_mcp_http.py"), http_log],
                                   stdout=subprocess.PIPE, text=True)
    atexit.register(http_server.kill)
    http_base = f"http://127.0.0.1:{http_server.stdout.readline().strip()}"
    remote = ctx.api("POST", "/api/library/mcp", {"name": "e2e-http", "description": "HTTP 路由测试", "transport": "http",
                                                  "url": f"{http_base}/mcp", "vars": [{"name": "Authorization", "value": "Bearer good"}]})
    def prepared(item_id):
        item = next(i for i in ctx.api("GET", "/api/library")["items"] if i["id"] == item_id)
        return item.get("warm") if (item.get("warm") or {}).get("state") in ("ready", "failed") else None
    warm = wait(lambda: prepared(routed["id"]), "新添加的 MCP 服务器预先准备完", 60)
    c(warm.get("state") == "ready" and warm.get("tools", 0) > 0, f"添加后预先启动一次并数出工具：{warm}")
    warm = wait(lambda: prepared(remote["id"]), "新添加的 HTTP 服务器预先连接完", 60)
    c(warm.get("state") == "ready" and warm.get("tools") == 2, f"HTTP 服务器添加后预先连接一次并数出工具：{warm}")

    # The default team, bound to the agents: PMO leads on Codex, the engineer and the qa on Claude Code.
    roles = {r["id"]: r for r in ctx.api("GET", "/api/setup")["roles"]}
    ctx.api("POST", "/api/setup/roles", dict(roles["pmo"], connection="setup-codex", lead=True))
    ctx.api("POST", "/api/setup/roles", dict(roles["developer"], connection="setup-claude"))
    ctx.api("POST", "/api/setup/roles", dict(roles["qa"], connection="setup-claude"))
    roles = {r["id"]: r for r in ctx.api("GET", "/api/setup")["roles"]}
    c(not any("resources" in r for r in roles.values()), "数字员工不再选资源")
    ws = ctx.api("POST", "/api/setup/workspaces", {"id": "setup", "name": "默认团队", "path": repo, "lead": "pmo", "brief": "共享资料：用 Python 3。",
                                                   "members": [{"role": "pmo"}, {"role": "developer"}, {"role": "qa"}]})
    c(ws["path"] == repo, "工作区保存规范化的绝对路径")
    insp = ctx.api("GET", f"/api/inspect?path={repo}")
    c(insp.get("git") is True, f"检查目录识别出 Git 仓库：{insp}")
    code, _ = ctx.api_error("DELETE", "/api/setup/roles/developer")
    c(code == 400, "被工作区使用的数字员工不能删除")

    team = Team(ctx, "setup", fake, repo, ws, {"developer": "developer", "qa": "qa"}, cs["setup-codex"], cs["setup-claude"])
    fake.script({"T1": ["write:hello.py"], "V1": ["approve"]})
    run = team.run("写一个 hello.py")
    fake.wait_call("goal 写一个 hello.py")
    c(True, "负责人（Codex 替身）收到 /goal")
    run.orchestrate(task("T1", "developer", owns=["hello.py"]), task("V1", "qa", verifies="T1"))
    board = run.read_board(lambda b: "[完成] V1" in b)
    c("[完成] T1" in board and "[完成] V1" in board, "研发完成，质量工程师（Claude 替身）独立验证完成")
    sessions = [line for line in fake.calls().splitlines() if line.startswith("mcp ") and f"/mcp/{run.id}/" in line and '/T1"' in line]
    servers = json.loads(sessions[-1][len("mcp "):]) if sessions else []
    c(sorted(x["name"] for x in servers) == ["janus", "janus-tools"], f"T1 的 session/new 只有看板和 janus-tools：{[x['name'] for x in servers]}")
    c(SECRET not in sessions[-1] and "Bearer good" not in sessions[-1] and http_base not in sessions[-1], "MCP 的地址和值都不进 session/new")
    router = next((x for x in servers if x["name"] == "janus-tools"), None)
    path = "/" + router["url"].split("/", 3)[3]
    def rpc(method, params, at=path):
        return ctx.api("POST", at, {"jsonrpc": "2.0", "id": 1, "method": method, "params": params})["result"]
    tools = rpc("tools/list", {})["tools"]
    names = [t["name"] for t in tools]
    c("e2e-route__cwd" in names and "e2e-http__whoami" in names and "e2e-route__hang" in names,
      f"MCP 路由列出命令和 HTTP 服务器的工具，分页的读完所有页：{names}")
    c("e2e-route__task_only" not in names, "只能以 task 方式调用的工具不列出")
    about = next((t["description"] for t in tools if t["name"] == "e2e-route__echo"), "")
    c(about == "[e2e-route] 原样返回 text", f"工具说明以服务器名开头：{about}")
    started = lambda: open(starts).read().count("start")  # noqa: E731
    c(started() == 1, f"会话打开和列工具都没有启动服务器，只有添加时预先准备的那一次（{started()} 次）")
    got = rpc("tools/call", {"name": "e2e-route__cwd", "arguments": {}})
    rpc("tools/call", {"name": "e2e-route__echo", "arguments": {"text": "again"}})
    c(started() == 2, f"第一次调用才启动，之后的调用用同一个进程（{started()} 次）")
    c(got["content"][0]["text"] == run.cwd(), "路由背后的服务器在 Run 的工作目录（worktree）里启动")
    seen = json.loads(rpc("tools/call", {"name": "e2e-http__whoami", "arguments": {}})["content"][0]["text"])
    c(seen.get("authorization") == "Bearer good", f"Janus 带着保存的 header 调用 HTTP 服务器：{seen}")
    lead_path = path.rsplit("/", 2)[0] + "/lead/tools"
    code = ctx.api_error("POST", lead_path, {"jsonrpc": "2.0", "id": 1, "method": "tools/list"})[0]
    c(code in (401, 403, 404), f"协作者的令牌不能冒充负责人使用路由（{code}）")
    calls_ = run.has("mcp.routed")
    c(calls_ and calls_[0]["data"]["tool"] == "e2e-route__cwd" and "arguments" not in calls_[0]["data"], "每次路由调用记一条 mcp.routed，不记参数")
    unknown = ctx.api("POST", path, {"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "e2e-route__nope", "arguments": {}}})
    c((unknown.get("error") or {}).get("code") == -32602, f"未知工具按 MCP 返回 -32602：{unknown}")
    detail = json.dumps(run.detail(), ensure_ascii=False)
    c(SECRET not in detail and SECRET not in ctx.api("GET", f"/api/runs/{run.id}/export", raw=True)[1].decode(), "Run 记录和导出中没有 MCP 的值")
    c("共享资料：用 Python 3。" in detail, "工作区共享资料进入负责人说明")
    run.finish("hello.py 写好了")
    c(run.status() == "delivered", "负责人结束后 Run 交付")

    if ctx.app:
        st = ctx.app.until(lambda s: {x["id"]: x["phase"] for x in s["connections"]}.get("setup-codex") == "ok"
                           and "setup" in [w["id"] for w in s["workspaces"]], "应用读到 Agent、数字员工和工作区", "reload", "open home:setup")
        c(True, "应用：设置里的 Agent 状态与服务一致（检查通过）")
        c({"pmo", "developer"} <= {r["id"] for r in st["roles"]}, "应用：读到默认数字员工")
        for tab in ("general", "agents", "library:skill", "library:mcp", "usage", "permissions", "updates"):
            shot = "settings-" + tab.replace(":", "-")
            ctx.app.do(f"open settings:{tab}", f"shot {shot}")
            c(_drawn(ctx, shot), f"应用：设置 › {tab} 画出来了")
        # Adding an Agent offers only Codex and Claude Code.
        state = ctx.app.do("agent-edit new claude apikey template", "shot agent-editor")
        c(_drawn(ctx, "agent-editor") and state["agentEditorEnv"] == template,
          "应用：新建 Claude 的自定义 API 时，「填入模板」带入的环境变量就是服务给的模板")
        ctx.app.do("close-sheet")
        state = ctx.app.do("agent-edit setup-claude")
        lines = state["claudeEnvironments"]["setup-claude"].splitlines()
        expected = cs["setup-claude"]["env"]
        c(set(lines) == {k if v is None else f"{k}={v}" for k, v in expected.items()}
          and "ANTHROPIC_AUTH_TOKEN" in lines and SECRET not in "\n".join(lines), "应用：设置列表里的 Agent 只有私有变量的名字")
        shown = ctx.app.until(lambda s: SECRET in (s.get("agentEditorEnv") or ""), "编辑器读到保存的值", "shot claude-env-edit")
        c(f"ANTHROPIC_AUTH_TOKEN={SECRET}" in shown["agentEditorEnv"].splitlines()
          and "ANTHROPIC_MODEL=deepseek-v4.1-flash" in shown["agentEditorEnv"].splitlines(),
          "应用：编辑 Agent 时环境变量全部显示值，令牌也在（本机优先）")
        ctx.app.do("close-sheet")
        # The MCP editor: a saved server as the form and as JSON, and a new one.
        for cmd, shot in (("mcp-edit e2e-mcp", "mcp-editor"), ("mcp-edit e2e-mcp json", "mcp-editor-json"), ("mcp-edit new", "mcp-editor-new"), ("mcp-edit catalog:github", "mcp-editor-catalog")):
            ctx.app.do(cmd, f"shot {shot}", "close-sheet")
            c(_drawn(ctx, shot), f"应用：MCP 编辑器 {cmd} 画出来了")
        # Digital employees' pictures: the default ones come with theirs; one of your own, cropped
        # and scaled by the app, replaces it; removing it shows the initials again.
        defaults = {"pmo", "generalist", "marketing", "product-manager", "developer", "qa", "friend"}
        st = ctx.app.until(lambda s: defaults <= set(s.get("avatars") or {}), "应用读到默认数字员工的头像", "reload")
        c(all(st["avatars"][r] == 256 for r in defaults), f"应用：7 位默认数字员工都带着 256 像素的头像（{st['avatars']}）")
        code, _ = ctx.api_error("PUT", "/api/roles/developer/avatar", {"data": base64.b64encode(b"GIF89a not a png").decode()})
        c(code == 400, f"不是 PNG 的头像被拒绝（{code}）")
        before = {r["id"]: r for r in ctx.api("GET", "/api/setup")["roles"]}["developer"]["avatar"]
        ctx.app.do(f"avatar developer {os.path.join(ROOT, 'backend', 'roles', 'friend.png')}")
        after = {r["id"]: r for r in ctx.api("GET", "/api/setup")["roles"]}["developer"]["avatar"]
        status, png, _ = ctx.api("GET", "/api/roles/developer/avatar", raw=True)
        side = int.from_bytes(png[16:20], "big") if png.startswith(b"\x89PNG") else 0
        c(after and after != before and status == 200 and side == 256, f"应用：换上自己的图后，数字员工的头像是裁好的 256 像素 PNG（{side}）")
        ctx.app.do("close-sheet", "role developer", "shot role-editor", "close-sheet", "workspace setup", "shot workspace-editor", "close-sheet")
        # The shared material has a sheet of its own, apart from the team's.
        st = ctx.app.do("brief setup", "shot brief-editor")
        c(st.get("editingBrief") == "setup" and st.get("editingWorkspace") is None and _drawn(ctx, "brief-editor"),
          "应用：共享资料在单独的表单里编辑，不打开工作区设置（brief-editor.png）")
        ctx.app.do("close-sheet")
        c(_drawn(ctx, "role-editor"), "应用：数字员工编辑器画出来了")
        st = ctx.app.do("avatar developer none")
        code, _ = ctx.api_error("GET", "/api/roles/developer/avatar")
        c(code == 404 and "developer" not in (st.get("avatars") or {}), f"移除头像后回到文字头像（{code}）")
        c(_drawn(ctx, "workspace-editor"), "应用：工作区编辑器画出来了")
        ctx.ui(c, run.id, lambda r, s: r["status"] == "delivered" and {t["id"]: t["status"] for t in r["tasks"]} == {"T1": "succeeded", "V1": "succeeded"}
               and next(t for t in r["tasks"] if t["id"] == "V1")["verdict"] == "approved",
               "Run 已交付，T1、V1 已完成，V1 判定通过", pages=("board", "team", "audit", "task:T1", "task:V1", "agent:lead"))
        listed = ctx.api("GET", "/api/runs?workspace=setup")["runs"]
        c(listed and all(r["config"]["workspace"]["id"] == "setup" for r in listed), "按工作区读取它的任务记录")
        # The workspace's page lists its task records, newest first; the sidebar lists the workspaces.
        want = [r["id"] for r in listed]
        st = ctx.app.until(lambda s: [r["id"] for r in s.get("histories", {}).get("setup", [])] == want,
                           "工作区页读到它的任务记录", "open home:setup")
        c([r["id"] for r in st["histories"]["setup"]] == want, f"应用：工作区页列出它的全部任务记录，从新到旧（{want}）")
        ctx.app.do("shot workspace-home")
        c(_drawn(ctx, "workspace-home"), "应用：工作区页画出来了（workspace-home.png）")


def _drawn(ctx, name):
    from lib import drawn
    return drawn(ctx.app.picture(name))
