"""Shared pieces of the end-to-end tests (docs/operations/20261031-testing.md#product_e2e).

A scratch Janus home and workspaces outside the checkout, a server on a free port of 127.0.0.1 (never the
person's own 7878; two test runs, for example two Janus tasks in their own worktrees, do not collide), the HTTP API the app uses, the board's MCP endpoint the agents use, the
scripted ACP agent standing in for Codex and Claude Code, and the app driven through its
snapshot command channel. Only the standard library: the scripts run with `python3 -I`.
"""

import itertools
import json
import os
import shutil
import signal
import subprocess
import threading
import time
import traceback
import urllib.error
import urllib.request

ROOT = subprocess.run(["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True, check=True,
                      cwd=os.path.dirname(os.path.abspath(__file__))).stdout.strip()
def free_addr():
    """A port nothing listens on right now. The system hands out ports from its ephemeral range,
    so it is never the person's own 7878."""
    import socket
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return f"127.0.0.1:{s.getsockname()[1]}"


ADDR = free_addr()
FAKE_AGENT = os.path.join(ROOT, "backend", "testdata", "fake_acp.py")
# The npm packages Janus runs for the built-in runtimes (backend/src/adapters.rs `ADAPTERS`).
ADAPTER_PACKAGES = {"codex": "@agentclientprotocol/codex-acp", "claude": "@agentclientprotocol/claude-agent-acp"}
STAND_IN_VERSION = "9.9.9"
# The names that may stand at the top of Janus home: `layout::TOP` in backend/src/layout.rs
# (docs/operations/20261031-deployment.md#layout).
HOME_TOP = {"config.toml", "janus.pid", "bin", "data", "work", "cache", "tmp", "logs", "backups"}


def home_db(home):
    """The database of a Janus home: `data/janus.db`, or at the top in a home an older release wrote."""
    new = os.path.join(home, "data", "janus.db")
    return new if os.path.exists(new) else os.path.join(home, "janus.db")


def paths_with(home, id_):
    """Every path in a Janus home whose name holds `id_`: what is left of an object after deleting it."""
    found = []
    for root, dirs, files in os.walk(home):
        found += [os.path.join(root, n) for n in dirs + files if id_ in n]
    return found


def foreign(home):
    """What stands at the top of a Janus home that is not Janus's."""
    return sorted(set(os.listdir(home)) - HOME_TOP - {".DS_Store"})

_print = threading.Lock()


def say(msg):
    with _print:
        print(msg, flush=True)


def run(cmd, **kw):
    return subprocess.run(cmd, check=True, **kw)


class Failure(Exception):
    """A check that the rest of a scenario depends on did not hold."""


class Checks:
    """The checks of one scenario: each is printed and kept for the report."""

    def __init__(self, name):
        self.name, self.items, self.error, self.seconds = name, [], None, 0.0

    def __call__(self, ok, what):
        ok = bool(ok)
        self.items.append((ok, what))
        say(f"  {'✓' if ok else '✗'} [{self.name}] {what}")
        return ok

    def must(self, ok, what):
        if not self(ok, what):
            raise Failure(what)

    @property
    def passed(self):
        return self.error is None and all(ok for ok, _ in self.items)


def wait(pred, what, timeout=60.0, every=0.2):
    """Poll `pred` until it returns something truthy; that value is returned."""
    end = time.time() + timeout
    last = None
    while time.time() < end:
        try:
            v = pred()
        except (OSError, ApiError, ValueError) as e:
            # The server restarting, a request refused for now, a file half written: retried.
            last, v = e, None
        if v:
            return v
        time.sleep(every)
    raise Failure(f"等待超时（{timeout:.0f} 秒）：{what}" + (f"；最后一次出错：{last}" if last else ""))


class ApiError(Exception):
    def __init__(self, code, text):
        super().__init__(f"{code} {text}")
        self.code, self.text = code, text


def http(addr, method, path, body=None, timeout=30, raw=False, headers=None):
    """One request to Janus; JSON in and out unless `raw`. HTTP errors raise ApiError."""
    data = None if body is None else (body if isinstance(body, bytes) else json.dumps(body).encode())
    req = urllib.request.Request(f"http://{addr}{path}", data=data, method=method,
                                 headers={"Content-Type": "application/json", **(headers or {})})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            out = r.read()
            if raw:
                return r.status, out, dict(r.headers)
            return json.loads(out) if out else None
    except urllib.error.HTTPError as e:
        raise ApiError(e.code, e.read().decode(errors="replace")) from None


class Registry:
    """A stand-in npm registry on 127.0.0.1: what `npm view <package> version` reads when the server
    runs with `npm_config_registry` pointing here, so its adapter update checks never reach npm.
    Every package's latest version is the stand-in adapters' until a scenario sets another
    (`latest[package]`); the packages asked about are recorded in `asked`."""

    def __init__(self):
        import http.server
        from urllib.parse import unquote
        reg = self
        self.latest, self.asked = {}, []

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                name = unquote(self.path.lstrip("/").split("?", 1)[0])
                reg.asked.append(name)
                v = reg.latest.get(name, STAND_IN_VERSION)
                body = json.dumps({"name": name, "dist-tags": {"latest": v}, "versions": {v: {"name": name, "version": v}},
                                   "time": {v: "2026-11-20T00:00:00.000Z"}}).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, *args):
                pass

        self.httpd = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.url = f"http://127.0.0.1:{self.httpd.server_address[1]}/"
        threading.Thread(target=self.httpd.serve_forever, daemon=True).start()


class Server:
    """A Janus server on a scratch home."""

    def __init__(self, binary, home, log, addr=ADDR, env=None):
        self.binary, self.home, self.log, self.addr, self.env = binary, home, log, addr, env or {}
        self.proc = None

    def start(self):
        env = dict(os.environ, JANUS_HOME=self.home, JANUS_ADDR=self.addr, **self.env)
        self.proc = subprocess.Popen([self.binary], env=env, stdout=open(self.log, "a"), stderr=subprocess.STDOUT)
        for _ in range(300):
            if self.proc.poll() is not None:
                raise Failure(f"{self.binary} 退出了（{self.proc.returncode}），见 {self.log}")
            try:
                return http(self.addr, "GET", "/api/meta", timeout=2)
            except (OSError, ApiError):
                time.sleep(0.2)
        raise Failure(f"{self.binary} 在 {self.addr} 上没有响应，见 {self.log}")

    def stop(self):
        if self.proc and self.proc.poll() is None:
            self.proc.send_signal(signal.SIGTERM)
            try:
                self.proc.wait(15)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait()


class Mcp:
    """The board's MCP endpoint as an agent reaches it: the URL Janus gave its session."""

    def __init__(self, url):
        self.url = url
        self.addr, self.path = url.split("://", 1)[1].split("/", 1)
        self.path = "/" + self.path

    def post(self, payload):
        """(HTTP status, JSON body or None)."""
        try:
            status, out, _ = http(self.addr, "POST", self.path, payload, raw=True, timeout=200)
            return status, (json.loads(out) if out else None)
        except ApiError as e:
            return e.code, None

    def tools(self):
        status, body = self.post({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})
        if status != 200:
            raise Failure(f"tools/list 返回 {status}")
        return [t["name"] for t in body["result"]["tools"]]

    def call(self, name, args=None):
        """(text, is_error) of a tool call."""
        status, body = self.post({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                                  "params": {"name": name, "arguments": args or {}}})
        if status != 200:
            raise Failure(f"{name} 返回 HTTP {status}")
        r = body["result"]
        return "".join(c.get("text", "") for c in r.get("content", [])), r.get("isError") is True


class Fake:
    """The scripted ACP agent's directory (backend/testdata/fake_acp.py, FAKE_DIR)."""

    def __init__(self, path, script=None):
        self.dir = path
        os.makedirs(path, exist_ok=True)
        self.script(script or {})

    def script(self, steps):
        with open(os.path.join(self.dir, "script.json"), "w") as f:
            json.dump(steps, f, ensure_ascii=False)

    def touch(self, name, text=""):
        with open(os.path.join(self.dir, name), "w") as f:
            f.write(text)

    def remove(self, name):
        p = os.path.join(self.dir, name)
        if os.path.exists(p):
            os.remove(p)

    def calls(self):
        try:
            with open(os.path.join(self.dir, "calls.log")) as f:
                return f.read()
        except FileNotFoundError:
            return ""

    def wait_call(self, text, timeout=60):
        return wait(lambda: text in self.calls(), f"calls.log 出现「{text}」", timeout)

    def urls(self, run):
        """The board URLs of every session of `run`, newest last."""
        out = []
        for line in self.calls().splitlines():
            if line.startswith("mcp "):
                for s in json.loads(line[4:]):
                    if s.get("name") == "janus" and f"/mcp/{run}/" in s.get("url", ""):
                        out.append(s["url"])
        return out

    def board(self, run, who, timeout=60):
        """The newest board endpoint of `who` (`lead` or a task id) in `run`."""
        url = wait(lambda: next((u for u in reversed(self.urls(run)) if u.endswith("/" + who)), None),
                   f"{who} 的看板地址", timeout)
        return Mcp(url)

    def finish_lead(self, text):
        self.touch("lead-go", text)


def git_repo(path, files=None):
    os.makedirs(path, exist_ok=True)
    for name, text in (files or {"README.md": "e2e\n"}).items():
        p = os.path.join(path, name)
        os.makedirs(os.path.dirname(p), exist_ok=True)
        with open(p, "w") as f:
            f.write(text)
    g = ["git", "-C", path, "-c", "user.name=e2e", "-c", "user.email=e2e@example.invalid"]
    run(g[:3] + ["init", "-q"])
    # Accepting a run commits its result as the repository's own identity.
    run(g[:3] + ["config", "user.name", "e2e"])
    run(g[:3] + ["config", "user.email", "e2e@example.invalid"])
    run(g + ["add", "-A"])
    run(g + ["commit", "-q", "-m", "init"])
    return os.path.realpath(path)


def stand_in_adapters(home, python):
    """Codex and Claude Code adapters that run the scripted agent instead of the real one.

    Janus starts a built-in runtime as `node <bin of the npm package>` from
    `<home>/cache/adapters/<id>/current`; here that script hands stdio to fake_acp.py, so the release
    build's own launch path (adapter lookup, Node, isolation, lead rules) runs unchanged."""
    for id_, package in ADAPTER_PACKAGES.items():
        d = os.path.join(home, "cache", "adapters", id_)
        pkg = os.path.join(d, STAND_IN_VERSION, "node_modules", package)
        os.makedirs(pkg, exist_ok=True)
        with open(os.path.join(pkg, "package.json"), "w") as f:
            json.dump({"name": package, "version": STAND_IN_VERSION, "bin": {package.split("/")[-1]: "shim.js"}}, f)
        with open(os.path.join(pkg, "shim.js"), "w") as f:
            f.write("// Stand-in adapter for Janus's end-to-end test: the scripted agent speaks ACP on stdio.\n"
                    "const { spawnSync } = require('child_process');\n"
                    f"const r = spawnSync({json.dumps(python)}, [{json.dumps(FAKE_AGENT)}], {{ stdio: 'inherit' }});\n"
                    "process.exit(r.status === null ? 1 : r.status);\n")
        link = os.path.join(d, "current")
        if not os.path.islink(link):
            os.symlink(STAND_IN_VERSION, link)


def real_node():
    """The node binary itself: Janus checks its version, and a shell wrapper on PATH is not one."""
    node = os.environ.get("JANUS_NODE") or shutil.which("node")
    if not node:
        raise Failure("PATH 里没有 node（ACP 适配器需要 Node 22.19 以上）")
    return subprocess.run([node, "-p", "process.execPath"], capture_output=True, text=True, check=True).stdout.strip()


class App:
    """The macOS app, driven through `JANUS_SNAPSHOT_DIR` (frontend/Sources/Janus/Core/Snapshots.swift).

    Commands go one batch at a time; each batch ends with `state <name>`, and the app writing
    `<name>.json` is the acknowledgement that the whole batch ran."""

    # Several apps run at once in one test: each keeps its local settings in a suite of its own.
    count = itertools.count(1)

    def __init__(self, binary, home, addr, snap_dir, log, env=None):
        self.dir, self.n, self.lock = snap_dir, 0, threading.Lock()
        os.makedirs(snap_dir, exist_ok=True)
        self.suite = f"janus-e2e-{os.getpid()}-{next(App.count)}"
        e = dict(os.environ, JANUS_HOME=home, JANUS_ADDR=addr, JANUS_SNAPSHOT_DIR=snap_dir,
                 JANUS_DEFAULTS_SUITE=self.suite, **(env or {}))
        self.proc = subprocess.Popen([binary], env=e, stdout=open(log, "a"), stderr=subprocess.STDOUT)

    def do(self, *lines, timeout=60):
        """Run commands and return the state the app writes after them."""
        with self.lock:
            if self.proc.poll() is not None:
                raise Failure(f"应用已退出（{self.proc.returncode}）")
            self.n += 1
            name = f"state-{self.n}"
            cmd = os.path.join(self.dir, "cmd")
            wait(lambda: not os.path.exists(cmd) or os.path.getsize(cmd) == 0, "应用读走上一批命令", timeout)
            with open(cmd, "w") as f:
                f.write("\n".join(list(lines) + [f"state {name}"]) + "\n")
            out = os.path.join(self.dir, name + ".json")
            wait(lambda: os.path.exists(out) and os.path.getsize(out) > 0, f"应用执行 {' / '.join(lines) or 'state'}", timeout)
            with open(out) as f:
                return json.load(f)

    def until(self, pred, what, *lines, timeout=60):
        """Repeat the commands until the state satisfies `pred`; the last state is returned."""
        end, st = time.time() + timeout, None
        while time.time() < end:
            st = self.do(*lines, timeout=max(5, end - time.time()))
            if pred(st):
                return st
            time.sleep(0.5)
        shown = (st or {}).get("run") or st
        raise Failure(f"应用状态等待超时：{what}；最后的状态：{json.dumps(shown, ensure_ascii=False)[:2500]}")

    def picture(self, name):
        return os.path.join(self.dir, name + ".png")

    def quit(self):
        if self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(10)
            except subprocess.TimeoutExpired:
                self.proc.kill()
        subprocess.run(["defaults", "delete", self.suite], capture_output=True)


def drawn(path):
    """A picture of a real window that is not a single flat colour (what a blank window gives).
    Without the main window open, `shot` takes the small menu bar window: too small to pass."""
    if not os.path.exists(path) or os.path.getsize(path) < 2000:
        return False
    # sips reports the size; a flat picture compresses to almost nothing for its size.
    out = subprocess.run(["sips", "-g", "pixelWidth", "-g", "pixelHeight", path], capture_output=True, text=True).stdout
    try:
        w = int(out.split("pixelWidth:")[1].split()[0])
        h = int(out.split("pixelHeight:")[1].split()[0])
    except (IndexError, ValueError):
        return False
    return w >= 300 and h >= 200 and os.path.getsize(path) > w * h / 400


def run_scenario(fn, ctx, name):
    """Run one scenario; an exception ends it as failed and is kept for the report."""
    c = Checks(name)
    t0 = time.time()
    try:
        fn(ctx, c)
    except Failure as e:
        c.error = str(e)
        say(f"  ✗ [{name}] 中止：{e}")
    except Exception as e:  # noqa: BLE001 - a crash in one scenario must not hide the others
        c.error = f"{type(e).__name__}: {e}\n{traceback.format_exc()}"
        say(f"  ✗ [{name}] 出错：{type(e).__name__}: {e}")
    c.seconds = time.time() - t0
    return c


def write_report(path, title, results, notes=()):
    lines = [f"# {title}", ""]
    lines += list(notes) + [""] if notes else []
    passed = sum(1 for r in results if r.passed)
    lines.append(f"场景：{passed}/{len(results)} 通过")
    lines.append("")
    for r in results:
        lines.append(f"## {'✓' if r.passed else '✗'} {r.name}（{r.seconds:.0f} 秒）")
        lines.append("")
        for ok, what in r.items:
            lines.append(f"- {'✓' if ok else '✗'} {what}")
        if r.error:
            lines.append("")
            lines.append("```text")
            lines.append(r.error.rstrip())
            lines.append("```")
        lines.append("")
    with open(path, "w") as f:
        f.write("\n".join(lines))

