#!/usr/bin/env python3
"""Product end-to-end test: the real server and app, with scripted agents standing in for Codex
and Claude Code (docs/operations/20261031-testing.md#product_e2e).

Builds the server and the app from this checkout (or uses dist/Janus.app with `--packaged`),
starts the server on a scratch Janus home at 127.0.0.1:7979, installs stand-in adapters that run
backend/testdata/fake_acp.py, and plays every user journey through the HTTP API the app uses and
the board MCP endpoint the agents use. The app runs against the same server; after each journey
its derived state is compared with what the server recorded, and its pages are pictured for review.

Usage: e2e/product.py [--packaged] [--real] [--only NAME ...] [--no-app] [--keep] [--list]
`--real` adds the smoke test with the real Codex and Claude Code (network, tokens; see real.py).
"""

import argparse
import concurrent.futures
import os
import shutil
import subprocess
import sys
import tempfile
import time

# Imports from e2e/ must not leave __pycache__ in the checkout (AGENTS.md rule 9).
sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from lib import ADDR, ROOT, App, Failure, Server, real_node, run, run_scenario, say, stand_in_adapters, write_report  # noqa: E402
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
    say("构建服务（cargo build）…")
    run(["cargo", "build", "--locked", "-q"], cwd=os.path.join(ROOT, "backend"))
    say("构建应用（swift build）…")
    run(["swift", "build", "-q"], cwd=os.path.join(ROOT, "frontend"))
    app_bin = subprocess.run(["swift", "build", "--show-bin-path"], cwd=os.path.join(ROOT, "frontend"),
                             capture_output=True, text=True, check=True).stdout.strip()
    return os.path.join(ROOT, "backend", "target", "debug", "janus"), os.path.join(app_bin, "Janus"), real_node()


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--packaged", action="store_true", help="test dist/Janus.app (built by just app) instead of the source build")
    p.add_argument("--real", action="store_true", help="also run the smoke test with the real Codex and Claude Code")
    p.add_argument("--only", action="append", help="run only these scenarios (repeatable)")
    p.add_argument("--no-app", action="store_true", help="skip the app: API and agents only")
    p.add_argument("--keep", action="store_true", help="keep the scratch homes and workspaces")
    p.add_argument("--list", action="store_true", help="list the scenarios")
    a = p.parse_args()
    if a.list:
        for s in SCENARIOS:
            say(f"{s.name:<24} {'[串行] ' if s.serial else ''}{'[--real] ' if s.real else ''}{s.doc}")
        return
    chosen = [s for s in SCENARIOS if (not s.real or a.real) and (not a.only or s.name in a.only)]
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
    server = Server(server_bin, home, os.path.join(report_dir, "janus.log"), env={"JANUS_NODE": node})
    ctx = scenarios.Ctx(tmp=tmp, home=home, addr=ADDR, server=server, report=report_dir, python=sys.executable,
                        node=node, server_bin=server_bin, app_bin=app_bin, packaged=a.packaged)
    results = []
    t0 = time.time()
    try:
        meta = server.start()
        say(f"服务 {meta.get('version')}（数据版本 {meta.get('dataVersion')}）在 {ADDR}")
        if not a.no_app:
            ctx.app = App(app_bin, home, ADDR, os.path.join(report_dir, "app"), os.path.join(report_dir, "app.log"))
            ctx.app.until(lambda s: s.get("connections") is not None, "应用连上服务", timeout=90)
        first = [s for s in chosen if s.first]
        parallel = [s for s in chosen if not s.first and not s.serial]
        serial = [s for s in chosen if not s.first and s.serial]
        for s in first:
            say(f"\n== {s.name}：{s.doc}")
            results.append(run_scenario(s.fn, ctx, s.name))
        if parallel:
            say(f"\n== 并行运行 {len(parallel)} 个场景")
            with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
                results += list(pool.map(lambda s: run_scenario(s.fn, ctx, s.name), parallel))
        for s in serial:
            say(f"\n== {s.name}：{s.doc}")
            results.append(run_scenario(s.fn, ctx, s.name))
    except Failure as e:
        say(f"\n✗ 无法开始：{e}")
        results.append(_setup_failure(str(e)))
    finally:
        if ctx.app:
            ctx.app.quit()
        server.stop()
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
    say(f"报告：{report}")
    if not a.no_app:
        say(f"截图：{shots}（看一遍新功能涉及的页面）")
    sys.exit(1 if failed else 0)


def _setup_failure(error):
    from lib import Checks
    c = Checks("启动")
    c.error = error
    return c


if __name__ == "__main__":
    main()
