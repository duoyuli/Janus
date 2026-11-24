#!/usr/bin/env python3
"""`just e2e`: every check a requirement needs before it is done, side by side
(docs/operations/20261031-testing.md#e2e).

Four steps share nothing but the machine, so they run at once instead of one after another: the
static checks and the orchestrator's tests (one cargo step: they share a build directory), the
app's tests, the product end-to-end test and the data upgrade test. Each writes its own log in a
temporary directory; this prints each step as it ends, with its time, the end of the log of any
that failed, and the product test's summary. Arguments go to the product test.

Usage: e2e/all.py [product test arguments …]
"""

import os
import subprocess
import sys
import tempfile
import threading
import time

sys.dont_write_bytecode = True
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def main():
    args = sys.argv[1:]
    py = sys.executable
    steps = [
        ("格式、静态检查和后端测试", "just lint && just test"),
        ("应用测试", "just test-mac"),
        ("产品端到端测试", " ".join([py, "-I", "e2e/product.py"] + args)),
        ("数据升级测试", f"{py} -I e2e/upgrade.py"),
    ]
    logs = tempfile.mkdtemp(prefix="janus-e2e-all-")
    print(f"同时运行 {len(steps)} 步，日志在 {logs}", flush=True)
    results, lock, t0 = {}, threading.Lock(), time.time()

    def step(n, name, cmd):
        log = os.path.join(logs, f"{n}.log")
        start = time.time()
        with open(log, "w") as f:
            code = subprocess.run(cmd, shell=True, cwd=ROOT, stdout=f, stderr=subprocess.STDOUT).returncode
        with lock:
            results[name] = (code, time.time() - start, log)
            print(f"{'✓' if code == 0 else '✗'} {name}（{time.time() - start:.0f} 秒）", flush=True)

    threads = [threading.Thread(target=step, args=(n, name, cmd)) for n, (name, cmd) in enumerate(steps, 1)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()

    for name, _ in steps:
        code, _, log = results[name]
        with open(log) as f:
            lines = f.read().splitlines()
        if name == "产品端到端测试":
            # Its own summary: scenarios passed, the failed ones, the report and the rerun command.
            tail = lines[next((i for i in range(len(lines) - 1, -1, -1) if lines[i].startswith("产品端到端测试：")), max(0, len(lines) - 20)):]
            print(f"\n== {name}\n" + "\n".join(tail))
        elif code != 0:
            print(f"\n== {name}没通过，日志最后 40 行（{log}）：\n" + "\n".join(lines[-40:]))
    failed = [name for name, _ in steps if results[name][0] != 0]
    print(f"\n端到端验证：{len(steps) - len(failed)}/{len(steps)} 步通过，用时 {time.time() - t0:.0f} 秒"
          + (f"；没通过：{'、'.join(failed)}" if failed else ""))
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
