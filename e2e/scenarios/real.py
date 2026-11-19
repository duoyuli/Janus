"""Smoke test with the real Codex and Claude Code (`--real`): network, tokens, and minutes.

A Janus home of its own on 127.0.0.1:7981: the agents copy their model settings and credentials
from ~/.codex and ~/.claude the way 「复制本机配置」 does (the person's own directories are only
read), Janus installs the real ACP adapters from npm, a Codex lead and Claude Code collaborators
take a small goal on a copy of examples/calc, and the result is checked by running its tests.
"""

import os
import shutil
import subprocess
import time

from lib import ROOT, Failure, Server, git_repo, http, wait
from scenarios import scenario

ADDR = "127.0.0.1:7981"
GOAL = ("给 calc.py 增加 mod(a, b)：返回 a 除以 b 的余数，b 为 0 时抛出 ZeroDivisionError；"
        "在 test_calc.py 补上对应测试，并运行 python3 -m unittest 确认全部通过。")


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
        for id_, kind, source in (("real-codex", "codex", "~/.codex"), ("real-claude", "claude", "~/.claude")):
            api("POST", "/api/setup/connections", {"id": id_, "kind": kind, "name": id_, "auth": "import", "source": source})
            api("POST", f"/api/connections/{id_}/test")
        def checked():
            cs = {x["id"]: x for x in api("GET", "/api/setup")["connections"]}
            return cs if all(cs[i].get("phase") in ("ok", "error") for i in ("real-codex", "real-claude")) else None
        cs = wait(checked, "真实 Agent 的配置检查（含从 npm 安装适配器）", 600, every=2)
        for i in ("real-codex", "real-claude"):
            c.must(cs[i]["phase"] == "ok", f"{i} 配置检查通过（{cs[i].get('error') or 'ok'}）")
        adapters = {a["id"]: a for a in api("GET", "/api/adapters")["adapters"]}
        c(all(adapters[i]["current"] for i in ("codex", "claude")), f"Janus 从 npm 安装了适配器：codex {adapters['codex']['current']}，claude {adapters['claude']['current']}")

        roles = {r["id"]: r for r in api("GET", "/api/setup")["roles"]}
        api("POST", "/api/setup/roles", dict(roles["coordinator"], connection="real-codex", lead=True))
        api("POST", "/api/setup/roles", dict(roles["implementer"], connection="real-claude"))
        api("POST", "/api/setup/roles", dict(roles["reviewer"], connection="real-claude"))
        api("POST", "/api/setup/workspaces", {"id": "calc", "name": "calc", "path": repo, "lead": "coordinator",
                                              "members": [{"role": "coordinator"}, {"role": "implementer"}, {"role": "reviewer"}]})
        run = api("POST", "/api/runs", {"goal": GOAL, "workspace": "calc"})["id"]
        c(True, f"提交目标（Run {run}）")
        status = _drive(api, run, c, timeout=30 * 60)
        c.must(status == "delivered", f"Run 交付（状态 {status}）")
        events = api("GET", f"/api/runs/{run}")["events"]
        done = {e.get("task") for e in events if e["type"] == "task.succeeded"}
        c(done, f"协作者完成了任务：{sorted(done)}")
        c("def mod" in open(os.path.join(repo, "calc.py")).read(), "calc.py 有 mod 函数")
        tests = subprocess.run([shutil.which("python3") or "python3", "-m", "unittest", "-q"], cwd=repo, capture_output=True, text=True)
        c(tests.returncode == 0, f"工作区的测试通过：{(tests.stderr or tests.stdout).strip().splitlines()[-1:]}")
        usage = api("GET", f"/api/runs/{run}/usage")
        c(usage["run"]["tokens"]["total"], f"真实 Agent 上报了用量：{usage['text']}")
        api("POST", f"/api/runs/{run}/actions", {"type": "accept"})
        c(True, "验收")
    finally:
        server.stop()
        # The copied credentials do not outlive the test, even with --keep.
        shutil.rmtree(os.path.join(home, "agents"), ignore_errors=True)


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
