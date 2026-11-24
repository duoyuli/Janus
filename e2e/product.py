#!/usr/bin/env python3
"""Product end-to-end test: the real server and app, with scripted agents standing in for Codex
and Claude Code (docs/operations/20261031-testing.md#product_e2e).

Builds the server and the app from this checkout (or uses dist/Janus.app with `--packaged`),
starts the server on a scratch Janus home at a free port of 127.0.0.1, installs stand-in adapters that run
backend/testdata/fake_acp.py, and plays every user journey through the HTTP API the app uses and
the board MCP endpoint the agents use. The app runs against the same server; after each journey
its derived state is compared with what the server recorded, and its pages are pictured for review.

Usage: e2e/product.py [--packaged] [--real [--real-from HOME]] [--only NAME ...] [--no-app] [--keep] [--list]
`--real` adds the smoke test with the real Codex and Claude Code (network, tokens; see real.py),
using a copy of the agent connections of the Janus home given by `--real-from` (~/.janus).
"""

import argparse
import concurrent.futures
import os
import queue
import shutil
import subprocess
import sys
import tempfile
import time

# Imports from e2e/ must not leave __pycache__ in the checkout (AGENTS.md rule 9).
sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from lib import ADDR, ROOT, App, Failure, Registry, Server, real_node, run, run_scenario, say, stand_in_adapters, write_report  # noqa: E402
import scenarios  # noqa: E402
from scenarios import SCENARIOS  # noqa: E402


def build(packaged):
    """(server binary, app binary, node for the adapters)."""
    if packaged:
        app = os.path.join(ROOT, "dist", "Janus.app")
        say("打包 dist/Janus.app（just app）…")
        run([os.path.join(ROOT, "frontend", "scripts", "package.sh"), "--no-dmg"], cwd=ROOT)
        return (os.path.join(app, "Contents", "MacOS", "janus-server"), os.path.join(app, "Contents", "MacOS", "Janus"),
                os.path.join(app, "Contents", "Resources", "node", "bin", "node"))
    say("构建服务（cargo build）和应用（swift build）…")
    # Neither needs the other: they build side by side.
    with concurrent.futures.ThreadPoolExecutor(2) as pool:
        builds = [pool.submit(run, ["cargo", "build", "--locked", "-q"], cwd=os.path.join(ROOT, "backend")),
                  pool.submit(run, ["swift", "build", "-q"], cwd=os.path.join(ROOT, "frontend"))]
        for b in builds:
            b.result()
    app_bin = subprocess.run(["swift", "build", "--show-bin-path"], cwd=os.path.join(ROOT, "frontend"),
                             capture_output=True, text=True, check=True).stdout.strip()
    return os.path.join(ROOT, "backend", "target", "debug", "janus"), os.path.join(app_bin, "Janus"), real_node()


# Scenarios side by side, each with an app of its own.
WORKERS = 6


class Apps:
    """The apps the scenarios drive: the first one for the scenarios that run alone, and one per
    worker for those side by side, so no scenario sees a page or sheet another one left open.
    Their pictures end up in one folder, `report/app`."""

    def __init__(self, binary, home, report, workers):
        self.binary, self.home, self.report, self.workers = binary, home, report, workers
        self.idle = queue.Queue()
        self.all = []
        self.starting = None

    def _new(self, n):
        app = App(self.binary, self.home, ADDR, os.path.join(self.report, "app" if n == 1 else f"app-{n}"),
                  os.path.join(self.report, "app.log" if n == 1 else f"app-{n}.log"))
        self.all.append(app)
        app.until(lambda s: s.get("connections") is not None, "应用连上服务", timeout=90)
        return app

    def first(self):
        if not self.workers:
            return None
        app = self._new(1)
        self.idle.put(app)
        return app

    def start_rest(self):
        """Start the other apps in the background; `run` waits for them."""
        if self.workers > 1:
            pool = concurrent.futures.ThreadPoolExecutor(self.workers - 1)
            self.starting = [pool.submit(self._new, n) for n in range(2, self.workers + 1)]
            pool.shutdown(wait=False)

    def run(self, s, ctx):
        if not self.workers:
            return run_scenario(s.fn, ctx.with_app(None), s.name)
        if self.starting:
            for f in self.starting:
                try:
                    self.idle.put(f.result())
                except Failure as e:
                    say(f"  ✗ 多开的应用没能启动：{e}")
            self.starting = []
        app = self.idle.get()
        try:
            return run_scenario(s.fn, ctx.with_app(app), s.name)
        finally:
            if app.proc.poll() is not None:
                # A scenario that crashed its app leaves the next one a fresh app, not a dead one.
                say(f"  ⚠ [{s.name}] 用过的应用已退出，换一个新的")
                n = len(self.all) + 1
                try:
                    app = self._new(n)
                except Failure as e:
                    say(f"  ✗ 新的应用没能启动：{e}")
            self.idle.put(app)

    def quit(self):
        for f in self.starting or []:
            f.cancel()
        for app in self.all:
            app.quit()
        # One folder of pictures to look through: report/app.
        shots = os.path.join(self.report, "app")
        for app in self.all:
            if app.dir != shots and os.path.isdir(app.dir):
                for name in os.listdir(app.dir):
                    if name.endswith(".png"):
                        shutil.move(os.path.join(app.dir, name), os.path.join(shots, name))
                shutil.rmtree(app.dir, ignore_errors=True)


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--packaged", action="store_true", help="test dist/Janus.app (built by just app) instead of the source build")
    p.add_argument("--real", action="store_true", help="also run the smoke test with the real Codex and Claude Code")
    p.add_argument("--real-from", default="~/.janus", help="Janus home whose agent connections the real smoke test copies")
    p.add_argument("--only", action="append", help="run only these scenarios (repeatable)")
    p.add_argument("--no-app", action="store_true", help="skip the app: API and agents only")
    p.add_argument("--keep", action="store_true", help="keep the scratch homes and workspaces")
    p.add_argument("--list", action="store_true", help="list the scenarios")
    a = p.parse_args()
    if a.list:
        for s in SCENARIOS:
            say(f"{s.name:<24} {'[自带服务] ' if s.own else ''}{'[--real] ' if s.real else ''}{s.doc}")
        return
    names = set(a.only or [])
    # A scenario that reads what another one made brings that one along.
    names |= {s.needs for s in SCENARIOS if s.name in names and s.needs}
    chosen = [s for s in SCENARIOS if (not s.real or a.real) and (not a.only or s.name in names)]
    unknown = set(a.only or []) - {s.name for s in SCENARIOS}
    if unknown:
        sys.exit(f"没有这些场景：{', '.join(sorted(unknown))}（--list 列出全部）")

    server_bin, app_bin, node = build(a.packaged)
    tmp = tempfile.mkdtemp(prefix="janus-e2e-")
    report_dir = os.path.join(tmp, "report")
    os.makedirs(report_dir)
    home = os.path.join(tmp, "home")
    os.makedirs(home)
    say(f"临时目录：{tmp}")
    stand_in_adapters(home, sys.executable)
    # The server's adapter update checks ask a stand-in registry, never npm (the --real smoke test
    # runs its own server, which installs from npm).
    registry = Registry()
    # Scenarios run side by side and leave runs working; enough slots that none of them waits in the
    # dispatch queue by accident (the queue scenario restarts the server with one slot).
    # The person's own home folder as the server sees it (本机环境): a stand-in, never the real one.
    host_home = os.path.join(tmp, "host-home")
    os.makedirs(host_home)
    env = {"JANUS_NODE": node, "npm_config_registry": registry.url, "JANUS_MAX_RUNNING_RUNS": "32", "JANUS_HOST_HOME": host_home}
    server = Server(server_bin, home, os.path.join(report_dir, "janus.log"), env=env)
    ctx = scenarios.Ctx(tmp=tmp, home=home, host_home=host_home, addr=ADDR, server=server, report=report_dir, python=sys.executable, registry=registry,
                        node=node, server_bin=server_bin, app_bin=app_bin, packaged=a.packaged, no_app=a.no_app, server_env=env,
                        real_from=os.path.expanduser(a.real_from))
    results = []
    t0 = time.time()
    first = [s for s in chosen if s.first]
    parallel = [s for s in chosen if not s.first and not s.own]
    own = [s for s in chosen if s.own]
    apps = Apps(app_bin, home, report_dir, workers=0 if a.no_app else min(WORKERS, max(1, len(parallel))))
    # The scenarios with a server and app of their own (the real agents' smoke test above all, the
    # longest of them) run from the start, beside everything below.
    side = concurrent.futures.ThreadPoolExecutor(max_workers=max(1, len(own)))
    beside = [side.submit(run_scenario, s.fn, ctx.with_app(None), s.name) for s in own]
    for s in own:
        say(f"\n== {s.name}（自带服务，和其他场景同时跑）：{s.doc}")
    try:
        meta = server.start()
        say(f"服务 {meta.get('version')}（数据版本 {meta.get('dataVersion')}）在 {ADDR}")
        main_app = apps.first()
        ctx.app = main_app
        # The other apps start while the first scenarios run; each scenario side by side borrows one.
        apps.start_rest()
        for s in first:
            say(f"\n== {s.name}：{s.doc}")
            results.append(run_scenario(s.fn, ctx, s.name))
        if parallel:
            say(f"\n== 并行运行 {len(parallel)} 个场景，各用自己的应用（{apps.workers or 0} 个）")
            with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, apps.workers)) as pool:
                results += list(pool.map(lambda s: apps.run(s, ctx), parallel))
    except Failure as e:
        say(f"\n✗ 无法开始：{e}")
        results.append(_setup_failure(str(e)))
    finally:
        apps.quit()
        server.stop()
        results += [f.result() for f in beside]
        side.shutdown()
    # Every scenario wrote into this one home; nothing landed outside the layout.
    results.append(_home_layout(home))
    elapsed = time.time() - t0
    report = os.path.join(report_dir, "report.md")
    shots = os.path.join(report_dir, "app")
    write_report(report, "Janus 产品端到端测试", results, [
        f"- 构建：{'dist/Janus.app' if a.packaged else '源码'}；用时 {elapsed:.0f} 秒",
        f"- 服务日志：{os.path.join(report_dir, 'janus.log')}",
        f"- 应用截图：{shots if not a.no_app else '（--no-app，未运行应用）'}",
    ])
    if not a.keep:
        for name in os.listdir(tmp):
            if name != "report":
                shutil.rmtree(os.path.join(tmp, name), ignore_errors=True)
    failed = [r for r in results if not r.passed]
    say(f"\n产品端到端测试：{len(results) - len(failed)}/{len(results)} 个场景通过，用时 {elapsed:.0f} 秒")
    for r in failed:
        say(f"  ✗ {r.name}：" + (r.error.splitlines()[0] if r.error else "; ".join(w for ok, w in r.items if not ok)))
    again = [r.name for r in failed if r.name in {s.name for s in SCENARIOS}]
    if again:
        flags = "".join(f" {f}" for f, on in (("--packaged", a.packaged), ("--real", a.real), ("--no-app", a.no_app)) if on)
        say("只重跑失败的场景：just e2e-product" + flags + "".join(f" --only {n}" for n in again))
    say(f"报告：{report}")
    if not a.no_app:
        say(f"截图：{shots}（看一遍新功能涉及的页面）")
    sys.exit(1 if failed else 0)


def _home_layout(home):
    from lib import Checks, foreign
    c = Checks("Janus home")
    c(not foreign(home), f"所有场景跑完后，Janus home 顶层只有布局声明过的名字：{sorted(os.listdir(home))}")
    return c


def _setup_failure(error):
    from lib import Checks
    c = Checks("启动")
    c.error = error
    return c


if __name__ == "__main__":
    main()
