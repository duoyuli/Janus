"""Smoke test with the real Codex and Claude Code (`--real`): network, tokens, and minutes.

A Janus home of its own on a free port of 127.0.0.1. The agents are a copy of the person's working setup:
the Codex and Claude Code connections of their Janus home (`--real-from`, ~/.janus) with the same
settings, so 「复制本机配置」 copies from the same source directories again, and secrets the API never
returns are copied file to file. Nothing in that home is changed. Janus installs the real ACP
adapters from npm, a Codex lead and Claude Code collaborators take a small goal on a copy of
examples/calc, and the result is checked by running its tests.
"""

import json
import os
import shutil
import sqlite3
import subprocess
import time

from lib import ROOT, Failure, Server, free_addr, git_repo, http, wait
from scenarios import scenario

ADDR = free_addr()
# The goal is small enough for the lead alone, which would then do it itself; this smoke test is
# about a real lead working with real collaborators, so the goal asks for delegation.
GOAL = ("给 calc.py 增加 mod(a, b)：返回 a 除以 b 的余数，b 为 0 时抛出 ZeroDivisionError；"
        "在 test_calc.py 补上对应测试，并运行 python3 -m unittest 确认全部通过。"
        "这是一次团队协作演练：请把实现交给研发协作者，再请质量协作者独立验证，你自己不要改文件。"
        "质量协作者开始验证前，先调用 janus-tools 里的 probe__echo 工具，text 写「开始验证」。")
# More skills for everyone than load directly, so the agents get the janus-skills router.
ROUTED_SKILLS = [f"e2e-skill-{i}" for i in range(11)]


@scenario("real", serial=True, real=True)
def real(ctx, c):
    """真实 Agent 冒烟：Codex 负责人带 Claude Code 协作者，在 examples/calc 的副本上完成一个小目标并通过测试"""
    home = ctx.path("real", "home")
    src = os.path.join(ROOT, "examples", "calc")
    files = {n: open(os.path.join(src, n)).read() for n in ("calc.py", "test_calc.py", "README.md", ".gitignore")}
    repo = git_repo(ctx.path("real", "calc"), files)
    server = Server(ctx.server_bin, home, os.path.join(ctx.report, "janus-real.log"), addr=ADDR, env={"JANUS_NODE": ctx.node})
    server.start()
    api = lambda method, path, body=None: http(ADDR, method, path, body, timeout=60)  # noqa: E731
    try:
        ids = _copy_connections(ctx.real_from, home, api, c)
        def checked():
            cs = {x["id"]: x for x in api("GET", "/api/setup")["connections"]}
            return cs if all(cs[i].get("phase") in ("ok", "error") for i in ids.values()) else None
        cs = wait(checked, "真实 Agent 的配置检查（含从 npm 安装适配器）", 600, every=2)
        for i in ids.values():
            c.must(cs[i]["phase"] == "ok", f"{i} 配置检查通过（{cs[i].get('error') or 'ok'}）")
        adapters = {a["id"]: a for a in api("GET", "/api/adapters")["adapters"]}
        c(all(adapters[i]["current"] for i in ("codex", "claude")), f"Janus 从 npm 安装了适配器：codex {adapters['codex']['current']}，claude {adapters['claude']['current']}")

        # The library as a person would fill it: a command server and eleven skills for everyone.
        probe = api("POST", "/api/library/mcp", {"name": "probe", "description": "端到端测试用", "transport": "stdio",
                                                 "command": f"{shutil.which('python3') or 'python3'} {os.path.join(ROOT, 'backend', 'testdata', 'fake_mcp.py')}"})
        api("POST", f"/api/library/items/{probe['id']}/reach", {"reach": "everyone"})
        for name in ROUTED_SKILLS:
            api("POST", "/api/library/skills", {"name": name, "description": f"端到端测试用的 Skill {name}", "content": "只在被要求时使用。"})

        roles = {r["id"]: r for r in api("GET", "/api/setup")["roles"]}
        api("POST", "/api/setup/roles", dict(roles["coordinator"], connection=ids["codex"], lead=True))
        api("POST", "/api/setup/roles", dict(roles["implementer"], connection=ids["claude"]))
        api("POST", "/api/setup/roles", dict(roles["reviewer"], connection=ids["claude"]))
        api("POST", "/api/setup/workspaces", {"id": "calc", "name": "calc", "path": repo, "lead": "coordinator",
                                              "members": [{"role": "coordinator"}, {"role": "implementer"}, {"role": "reviewer"}]})
        run = api("POST", "/api/runs", {"goal": GOAL, "workspace": "calc"})["id"]
        c(True, f"提交目标（Run {run}）")
        status = _drive(api, run, c, timeout=30 * 60)
        c.must(status == "delivered", f"Run 交付（状态 {status}）")
        events = api("GET", f"/api/runs/{run}")["events"]
        done = {e.get("task") for e in events if e["type"] == "task.succeeded"}
        c(done, f"协作者完成了任务：{sorted(done)}")
        c(not any(e["type"] in ("lead.edited", "lead.routed") for e in events), "负责人派出子任务后没有自己改文件，也没有调用资源库工具")
        routed = [e for e in events if e["type"] == "mcp.routed" and (e.get("data") or {}).get("tool") == "probe__echo"]
        c(routed and not routed[0]["data"].get("error"), f"真实的 Claude Code 经 janus-tools 调用了资源库工具：{[e.get('task') for e in routed]}")
        # Both agents list the skills they loaded as commands (Codex `$<name>`, Claude Code
        # `janus:<name>`). The eleven skills sit hidden under janus-skills, so each agent must show
        # the router and none of them: Codex skips hidden folders when it walks a skill root
        # (HiddenDirectoryPolicy::Skip), Claude Code reads a plugin's skills one level deep.
        by_agent = {}
        for e in events:
            if e["type"] == "session.commands":
                names = [x.get("name", "") for x in (e.get("data") or {}).get("commands", [])]
                by_agent.setdefault("codex" if e.get("actor") == "lead" else "claude", set()).update(names)
        for agent, router in (("codex", "$janus-skills"), ("claude", "janus:janus-skills")):
            names = by_agent.get(agent, set())
            leaked = sorted(n for n in names if any(n.endswith(s) for s in ROUTED_SKILLS))
            c(router in names and not leaked,
              f"{agent} 只加载了 Skill 路由，没有单独加载被收起的 Skill（有 {router}：{router in names}，单独出现的：{leaked}，共 {len(names)} 个命令）")
        worktree = api("GET", f"/api/runs/{run}")["run"]["cwd"]
        c("def mod" in open(os.path.join(worktree, "calc.py")).read(), "任务的 worktree 里 calc.py 有 mod 函数")
        tests = subprocess.run([shutil.which("python3") or "python3", "-m", "unittest", "-q"], cwd=worktree, capture_output=True, text=True)
        c(tests.returncode == 0, f"worktree 里的测试通过：{(tests.stderr or tests.stdout).strip().splitlines()[-1:]}")
        usage = api("GET", f"/api/runs/{run}/usage")
        c(usage["run"]["tokens"]["total"], f"真实 Agent 上报了用量：{usage['text']}")
        api("POST", f"/api/runs/{run}/actions", {"type": "accept"})
        shown = subprocess.run(["git", "show", f"janus/{run}:calc.py"], cwd=repo, capture_output=True, text=True).stdout
        c("def mod" in shown, f"验收后结果提交到分支 janus/{run}")
    finally:
        server.stop()
        # The copied credentials do not outlive the test, even with --keep.
        shutil.rmtree(os.path.join(home, "agents"), ignore_errors=True)


# What a check derives or records; everything else is the connection's own settings.
DERIVED = {"agentInfo", "base", "check", "checkedAt", "createdAt", "elapsedMs", "env", "error", "errorLog",
           "importedEnv", "options", "reply", "status", "testStartedAt"}


def _copy_connections(src_home, home, api, c):
    """Copy the person's Codex and Claude Code connections (a checked one of each kind) into the
    test home and start their checks. Returns {kind: connection id}."""
    db_path = os.path.join(src_home, "janus.db")
    if not os.path.exists(db_path):
        raise Failure(f"{src_home} 中没有 Janus 数据，用 --real-from 指定一个配置好 Codex 和 Claude Code 的 Janus home")
    db = sqlite3.connect(f"file:{db_path}?mode=ro", uri=True)
    try:
        conns = [json.loads(d) for (d,) in db.execute("SELECT data FROM configs WHERE kind = 'connection' ORDER BY id")]
    finally:
        db.close()
    ids = {}
    for kind, label in (("codex", "Codex"), ("claude", "Claude Code")):
        mine = sorted((x for x in conns if x.get("kind") == kind), key=lambda x: x.get("status") != "ok")
        if not mine:
            raise Failure(f"{src_home} 中没有 {label} 的 Agent，先在 Janus 中添加并通过配置检查")
        cfg = mine[0]
        secrets = os.path.join(src_home, "agents", cfg["id"], "secrets.json")
        if os.path.exists(secrets):
            dest = os.path.join(home, "agents", cfg["id"])
            os.makedirs(dest, mode=0o700, exist_ok=True)
            shutil.copyfile(secrets, os.path.join(dest, "secrets.json"))
            os.chmod(os.path.join(dest, "secrets.json"), 0o600)
        body = {k: v for k, v in cfg.items() if k not in DERIVED}
        body["envText"] = "\n".join(f"{k}={v}" for k, v in (cfg.get("env") or {}).items())
        api("POST", "/api/setup/connections", body)
        api("POST", f"/api/connections/{cfg['id']}/test")
        source = f"，复制自 {cfg.get('source') or '默认目录'}" if cfg.get("auth") == "import" else ""
        c(True, f"拷贝 {label} 的 Agent「{cfg['id']}」（{cfg.get('auth')}{source}）")
        ids[kind] = cfg["id"]
    return ids


def _drive(api, run, c, timeout):
    """Wait for the run to settle, answering what reaches the person the way a cautious person
    would: the recommended or first option, approvals allowed once."""
    end, answered = time.time() + timeout, set()
    while time.time() < end:
        d = api("GET", f"/api/runs/{run}")
        status = d["run"]["status"]
        if status in ("delivered", "accepted", "failed", "cancelled", "interrupted"):
            return status
        closed = {(e.get("data") or {}).get("key") for e in d["events"] if e["type"] in (
            "approval.granted", "approval.denied", "input.answered", "question.answered", "decision.withdrawn")}
        for e in d["events"]:
            key = (e.get("data") or {}).get("key")
            if not key or key in closed or key in answered:
                continue
            data, body = e["data"], None
            if e["type"] == "question.escalated":
                body = {"type": "answer", "key": key, "task": e.get("task") or "",
                        "answer": data.get("recommend") or (data.get("options") or ["按你的建议"])[0]}
            elif e["type"] == "approval.requested":
                opt = next((o for o in data.get("options", []) if o.get("kind") == "allow_once"), (data.get("options") or [{}])[0])
                body = {"type": "approve", "key": key, "task": e.get("task") or "", "allow": True, "option": opt.get("optionId"), "label": opt.get("name")}
            elif e["type"] == "input.requested":
                body = {"type": "input", "key": key, "task": e.get("task") or "", "action": "decline"}
            if body:
                api("POST", f"/api/runs/{run}/actions", body)
                answered.add(key)
                c(True, f"替用户处理：{e['type']} → {body.get('answer') or body.get('label') or body.get('action')}")
        time.sleep(5)
    raise Failure(f"{timeout // 60} 分钟内 Run 没有结束")
