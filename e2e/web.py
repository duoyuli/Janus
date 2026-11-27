#!/usr/bin/env python3
"""Web end-to-end test: the web client in Chrome, on the real server it is built into
(docs/operations/20261031-testing.md#web_e2e).

Builds the web client (`just web`) and the server that embeds it (cargo build),
starts the server on a scratch Janus home at a free port of 127.0.0.1 with the stand-in agents,
makes runs that wait on the person (questions with and without quick answers, a delivery), then
runs the Playwright tests in web/e2e against it: Chrome is paired with a code taken as the Mac
app takes it, then each page's drawn state (`window.__janus.state()`)
is compared item by item with what the service returns, every page is pictured at desktop and
phone width, and what the tests click reaches the service. Playwright drives the Chrome installed
on this Mac; it downloads no browser.

Usage: e2e/web.py [--keep] [--built] [-- playwright arguments …]
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time

sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from lib import ADDR, ROOT, Failure, Registry, Server, real_node, run, say, stand_in_adapters, wait  # noqa: E402
import scenarios  # noqa: E402

WEB = os.path.join(ROOT, "web")


def build(built):
    """The server binary, with the web client built inside it (already built when `built`)."""
    if not built:
        say("构建 Web 版（just web）…")
        run(["just", "web"], cwd=ROOT)
    say("构建服务（cargo build）…")
    run(["cargo", "build", "--locked", "-q"], cwd=os.path.join(ROOT, "backend"))
    return os.path.join(ROOT, "backend", "target", "debug", "janus")


def prepare(ctx):
    """Runs that wait on the person, as the tests expect them (their ids go to the tests)."""
    t = ctx.team("web", members=("m1",))
    asked = t.run("选一个部署环境")
    lead = asked.lead()
    text, err = lead.call("ask_user", {"question": "部署到哪个环境？", "why": "需要用户的账号", "options": ["测试", "生产"], "recommend": "测试"})
    if err:
        raise Failure(f"ask_user 被拒绝：{text}")
    asked.wait_event("question.escalated", actor="lead")
    later = t.run("整理一份发布说明")
    text, err = later.lead().call("ask_user", {"question": "发布说明用中文还是英文？", "why": "看读者是谁"})
    if err:
        raise Failure(f"ask_user 被拒绝：{text}")
    later.wait_event("question.escalated", actor="lead")
    # Its own team: a lead told to finish finishes every lead on the team's scripted agent.
    delivered = ctx.team("web-notes", members=("m1",)).run("写一份笔记")
    delivered.lead()
    delivered.finish("目标已完成：笔记写好了")
    # A goal delivered with a subtask's evidence, a diff, a document and a timeline: the run page.
    rt = ctx.team("web-review", members=("m1",), script={"T1": ["write:notes.md"]})
    reviewed = rt.run("写一份笔记并交付审阅")
    reviewed.orchestrate(scenarios.task("T1", rt.member("m1")))
    reviewed.read_board(lambda b: "[完成] T1" in b)
    reviewed.finish("## 结论\n笔记写好了。\n\n## 需要你决定\n- 要不要发布？\n\n## 改了什么\nnotes.md 写了一份笔记")
    # Asking before what cannot be undone, each on its own team (a lead told to finish finishes
    # every lead of its team's agent): a goal at work to stop, a delivered one to delete, and one
    # delivered with lines the lead suggests for the shared material.
    stopping = ctx.team("web-stop", members=("m1",)).run("要停的任务")
    stopping.lead()
    gone = ctx.team("web-gone", members=("m1",)).run("要删的任务")
    gone.lead()
    gone.finish("目标已完成：删掉也无妨")
    suggested = ctx.team("web-brief", members=("m1",)).run("写定价页")
    suggested.lead()
    suggested.finish("定价页写好了。\n\n## 建议更新共享资料\n- 定价页在 docs/pricing.md\n- 免费版限 3 个项目")
    # Each one is in the inbox before the browser opens it.
    wait(lambda: {asked.id, later.id, delivered.id, reviewed.id} <= {e["run"] for e in ctx.api("GET", "/api/inbox?lang=zh")["open"]},
         "四个 Run 都进了收件箱", timeout=60)
    # Team memory the agents kept in `web`: one note each project edits or deletes.
    for note in ({"kind": "pitfall", "title": "端口被占用", "hook": "别用 7878", "body": "测试一律用空闲端口。"},
                 {"kind": "howto", "title": "怎么跑测试", "hook": "先装依赖再跑测试", "body": "运行 just setup，\n再运行 just test。"},
                 {"kind": "fact", "title": "要删的笔记", "hook": "删掉也无妨", "body": "网页上删掉它。"}):
        text, err = lead.call("remember", note)
        if err:
            raise Failure(f"remember 被拒绝：{text}")
    # A Loop on a schedule, asleep until it is called.
    loop = ctx.api("POST", "/api/workspaces/web/loops", {"name": "看 issue", "prompt": "看看有什么新 issue", "schedule": {"cron": "0 9 * * 1-5"}, "wakeNow": False})
    # A workspace whose lead finishes every goal: the goals the browser starts there run to
    # delivery. Its Agents pass their check, as a person's would, so its team can run and its
    # digital employees can be picked for a chat.
    fresh = ctx.team("web-new", members=("m1",))
    fresh.fake.touch("lead-go", "目标已完成：网页上提的目标")
    checked = (fresh.lead_conn["id"], fresh.member_conn["id"])
    for c in checked:
        ctx.api("POST", f"/api/connections/{c}/test")
    wait(lambda: all(x.get("phase") == "ok" for x in ctx.api("GET", "/api/setup")["connections"] if x["id"] in checked),
         "web-new 的 Agent 通过配置检查", timeout=90)
    # Conversations: a chat with a reply, and a goal whose tasks show a whole session (thinking,
    # plan, calls with a diff), ask for a permission and ask for a form.
    s = ctx.team("web-session", members=("m1", "m2", "m3"), files={"README.md": "e2e\n"},
                 script={"T1": ["rich"], "T2": ["permission"], "T3": ["elicit"]})
    chat = ctx.api("POST", "/api/chats", {"role": s.member("m1")})["id"]

    def chat_events():
        return ctx.api("GET", f"/api/runs/{chat}")["events"]

    wait(lambda: [e for e in chat_events() if e["type"] == "agent.status" and e["data"].get("turn") == "idle"], "对话打开，等用户先说", 60)
    ctx.api("POST", f"/api/runs/{chat}/actions", {"type": "message", "agent": "lead", "text": "你好"})
    wait(lambda: [e for e in chat_events() if e["type"] == "agent.message" and "回复：你好" in e["data"].get("text", "")], "数字员工回复", 60)
    goal = s.run("看得见的会话")
    goal.lead()
    goal.orchestrate(scenarios.task("T1", s.member("m1")), scenarios.task("T2", s.member("m2")), scenarios.task("T3", s.member("m3")))
    goal.wait_event("task.succeeded", task="T1")
    goal.wait_event("approval.requested", task="T2")
    goal.wait_event("input.requested", task="T3")
    # A folder to make a workspace of from the browser.
    folder = ctx.path("web-folder")
    return {"workspace": "web", "asked": asked.id, "later": later.id, "delivered": delivered.id, "reviewed": reviewed.id,
            "newWorkspace": "web-new", "loop": loop["id"], "loopRun": loop["run"], "folder": folder,
            "chat": chat, "goal": goal.id, "stopping": stopping.id, "gone": gone.id, "suggested": suggested.id}


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--keep", action="store_true", help="keep the scratch home")
    p.add_argument("--built", action="store_true", help="web/dist is already built (just e2e builds it first)")
    p.add_argument("playwright", nargs="*", help="arguments for playwright test (after --)")
    a = p.parse_args()
    server_bin = build(a.built)
    tmp = tempfile.mkdtemp(prefix="janus-web-e2e-")
    report = os.path.join(tmp, "report")
    home = os.path.join(tmp, "home")
    os.makedirs(report)
    os.makedirs(home)
    say(f"临时目录：{tmp}")
    stand_in_adapters(home, sys.executable)
    registry = Registry()
    env = {"JANUS_NODE": real_node(), "npm_config_registry": registry.url}
    server = Server(server_bin, home, os.path.join(report, "janus.log"), env=env)
    ctx = scenarios.Ctx(tmp=tmp, home=home, addr=ADDR, server=server, report=report, python=sys.executable)
    t0 = time.time()
    code = 1
    try:
        meta = server.start()
        say(f"服务 {meta.get('version')} 在 {ADDR}")
        data = prepare(ctx)
        say(f"准备好的数据：{data}")
        env = dict(os.environ, JANUS_WEB_URL=f"http://{ADDR}", JANUS_WEB_DATA=json.dumps(data), JANUS_WEB_REPORT=report)
        code = subprocess.run(["npx", "--no-install", "playwright", "test", "-c", "e2e/playwright.config.ts", *a.playwright], cwd=WEB, env=env).returncode
    except Failure as e:
        say(f"✗ 无法开始：{e}")
    finally:
        server.stop()
        registry.close()
    shots = os.path.join(report, "shots")
    say(f"\nWeb 端到端测试：{'通过' if code == 0 else '没通过'}，用时 {time.time() - t0:.0f} 秒")
    say(f"报告：{os.path.join(report, 'playwright')}（npx playwright show-report 打开）")
    say(f"截图：{shots}（桌面和手机宽度，看一遍新功能涉及的页面）")
    if not a.keep and code == 0:
        shutil.rmtree(home, ignore_errors=True)
    sys.exit(code)


if __name__ == "__main__":
    main()
