"""The product end-to-end scenarios and what they share (docs/operations/20261031-testing.md#product_e2e).

A scenario is a function `(ctx, c)` registered with `@scenario`: `ctx` is the running server (and
app), `c` records checks. Each one builds its own team (agents, digital employees, workspace and
a scripted agent directory) so scenarios run side by side on one server, each driving an app of
its own; the ones that restart a server or need other settings bring a server of their own
(`ctx.private`) and run from the start beside the rest.
"""

import contextlib
import copy
import json
import os
import shutil
import threading
import time

from lib import App, ApiError, Failure, Fake, Server, drawn, free_addr, git_repo, http, stand_in_adapters, wait


class Scenario:
    def __init__(self, fn, name, first, real, own, needs):
        self.fn, self.name, self.first, self.real, self.own, self.needs = fn, name, first, real, own, needs
        self.doc = (fn.__doc__ or "").strip().splitlines()[0]


SCENARIOS = []


def scenario(name, first=False, real=False, own=False, needs=None):
    """Register a scenario. `first` runs before all others (setup that others may rely on); `own`
    from the start, beside everything else (it brings its own server and app, `ctx.private`, and
    uses neither of the shared ones); `real` only with --real; `needs` names a scenario whose data
    it reads, chosen along with it."""
    def add(fn):
        SCENARIOS.append(Scenario(fn, name, first, real, own, needs))
        return fn
    return add


def task(id_, assignee, **extra):
    """An orchestration entry with one acceptance criterion (like flow_tests' `task`)."""
    t = {"id": id_, "title": id_, "instructions": f"{id_} 的说明", "assignee": assignee, "accept": [f"{id_} 的结果写清楚"]}
    t.update(extra)
    return t


class Ctx:
    def __init__(self, **kw):
        self.__dict__.update(kw)
        self.app = None
        self.lock = threading.Lock()

    @contextlib.contextmanager
    def private(self, name, settings=None, **env):
        """A Janus home, server and app of this scenario's own, set up like the shared ones but
        for `env` (and the app's local `settings`): for a scenario that restarts its server or needs
        other settings, so it runs beside the others instead of after them. Its pictures join the
        others in report/app."""
        home = self.path(name, "home")
        stand_in_adapters(home, self.python)
        addr = free_addr()
        server = Server(self.server_bin, home, os.path.join(self.report, f"janus-{name}.log"), addr=addr, env=dict(self.server_env, **env))
        app = None
        try:
            server.start()
            if not self.no_app:
                app = App(self.app_bin, home, addr, os.path.join(self.report, f"app-{name}"), os.path.join(self.report, f"app-{name}.log"),
                          settings=settings)
                app.until(lambda s: s.get("connections") is not None, "应用连上服务", timeout=90)
            c = copy.copy(self)
            c.home, c.addr, c.server, c.app = home, addr, server, app
            yield c
        finally:
            if app:
                app.quit()
                shots = os.path.join(self.report, "app")
                os.makedirs(shots, exist_ok=True)
                for f in os.listdir(app.dir):
                    if f.endswith(".png"):
                        shutil.move(os.path.join(app.dir, f), os.path.join(shots, f))
                shutil.rmtree(app.dir, ignore_errors=True)
            server.stop()

    def with_app(self, app):
        """This context for one scenario, driving `app`: scenarios side by side each drive an app
        of their own, so what one leaves open (a sheet, a page) never meets another."""
        c = copy.copy(self)
        c.app = app
        return c

    def api(self, method, path, body=None, **kw):
        return http(self.addr, method, path, body, **kw)

    def api_error(self, method, path, body=None, **kw):
        """The HTTP status of a request that must fail."""
        try:
            self.api(method, path, body, **kw)
        except ApiError as e:
            return e.code, e.text
        return 200, ""

    def path(self, *parts):
        p = os.path.join(self.tmp, *parts)
        os.makedirs(p, exist_ok=True)
        return p

    def team(self, name, lead="codex", members=("m1",), script=None, files=None, brief="", slots=None, fake_files=()):
        """A workspace with a lead on a stand-in `lead` adapter (codex or claude) and collaborators
        on the stand-in Claude Code adapter, all the scripted agent sharing one FAKE_DIR. Role ids
        are `<name>-<member>`."""
        fake = Fake(self.path(name, "fake"), script)
        for f in fake_files:
            fake.touch(f)
        repo = git_repo(self.path(name, "repo"), files)
        env = f"FAKE_DIR={fake.dir}"
        def connection(suffix, kind):
            auth = ({"auth": "apikey", "envText": env + "\nexport ANTHROPIC_AUTH_TOKEN=e2e-token"} if kind == "claude"
                    else {"auth": "apikey", "apiKey": "sk-e2e-not-a-key", "envText": env})
            return self.api("POST", "/api/setup/connections", {"id": f"{name}-{suffix}", "kind": kind,
                                                               "name": f"{name} {suffix}", **auth})
        lead_conn = connection(lead, lead)
        member_conn = connection("fake", "claude")
        lead_role = self.api("POST", "/api/setup/roles", {
            "id": f"{name}-lead", "name": f"{name} 负责人", "desc": "负责人", "connection": lead_conn["id"], "lead": True})
        roles = {}
        for m in members:
            roles[m] = self.api("POST", "/api/setup/roles", {
                "id": f"{name}-{m}", "name": f"{name} {m}", "desc": f"{m} 的职责", "connection": member_conn["id"]})["id"]
        ws = self.api("POST", "/api/setup/workspaces", {
            "id": name, "name": name, "path": repo, "lead": lead_role["id"], "brief": brief,
            "members": [{"role": lead_role["id"]}] + [dict({"role": r}, **({"slots": slots[m]} if slots and m in slots else {})) for m, r in roles.items()]})
        return Team(self, name, fake, repo, ws, roles, lead_conn, member_conn)

    def ui(self, c, run_id, pred, what, pages=(), timeout=60):
        """The app shows run `run_id` as `pred` expects; pictures each page in `pages`
        (`board`, `team`, `audit`, `task:<id>`, `agent:<id>`, or a full command)."""
        if not self.app:
            return None
        st = self.app.until(lambda s: (s.get("run") or {}).get("id") == run_id and s["run"]["loaded"] and pred(s["run"], s),
                            what, f"open run:{run_id}", timeout=timeout)
        c(True, f"应用：{what}")
        for page in pages:
            name = run_id + "-" + "".join(ch if ch.isalnum() else "-" for ch in page).strip("-")
            cmd = page if " " in page else f"page {page}"
            self.app.do(f"open run:{run_id}", cmd, f"shot {name}")
            c(drawn(self.app.picture(name)), f"应用：{page} 页画出来了（{name}.png）")
        return st


class Team:
    def __init__(self, ctx, name, fake, repo, ws, roles, lead_conn, member_conn):
        self.ctx, self.name, self.fake, self.repo, self.ws = ctx, name, fake, repo, ws
        self.roles, self.lead_conn, self.member_conn = roles, lead_conn, member_conn

    def member(self, m):
        """The member id (and orchestration assignee) of collaborator `m`."""
        return self.roles[m]

    def run(self, goal, **extra):
        r = self.ctx.api("POST", "/api/runs", dict({"goal": goal, "workspace": self.ws["id"]}, **extra))
        return RunH(self, r["id"])


class RunH:
    """One run, seen through the API (as the app sees it) and the lead's board endpoint."""

    def __init__(self, team, id_):
        self.team, self.ctx, self.id, self.fake = team, team.ctx, id_, team.fake

    def detail(self):
        return self.ctx.api("GET", f"/api/runs/{self.id}")

    def cwd(self):
        """Where the run works: its own worktree on a git repository, otherwise the directory."""
        return self.detail()["run"]["cwd"]

    def events(self):
        return self.detail()["events"]

    def status(self):
        return self.detail()["run"]["status"]

    def has(self, kind, **match):
        return [e for e in self.events() if e["type"] == kind and all(_match(e, k, v) for k, v in match.items())]

    def wait_event(self, kind, timeout=60, **match):
        """The first `kind` event whose fields (`task`, `to`, `actor`, or `data__<key>`) match."""
        return wait(lambda: self.has(kind, **match), f"{self.id} 的 {kind} {match or ''}", timeout)[0]

    def wait_status(self, status, timeout=60):
        return wait(lambda: self.status() == status, f"{self.id} 进入 {status}（现在是 {self.status()}）", timeout)

    def act(self, type_, **fields):
        return self.ctx.api("POST", f"/api/runs/{self.id}/actions", dict({"type": type_}, **fields))

    def act_error(self, type_, **fields):
        return self.ctx.api_error("POST", f"/api/runs/{self.id}/actions", dict({"type": type_}, **fields))

    def lead(self, timeout=60):
        """The lead's board endpoint (a new one after every restart of its session)."""
        self.fake.wait_call("goal ", timeout)
        return self.fake.board(self.id, "lead", timeout)

    def orchestrate(self, *tasks):
        text, err = self.lead().call("orchestrate", {"tasks": list(tasks)})
        if err:
            raise Failure(f"编排被拒绝：{text}")
        return text

    def read_board(self, until, on=None, timeout=90):
        """The lead reads the board with `wait` until `until(text so far)`; `on(text)` may act
        on what arrived (answer, retry …). Returns everything read."""
        board, end = "", time.time() + timeout
        lead = self.lead()
        while time.time() < end:
            text, err = lead.call("wait", {"timeout_sec": 2})
            if err:
                raise Failure(f"wait 出错：{text}")
            board += text + "\n"
            if on:
                on(board)
            if until(board):
                return board
        seen = sorted({line for line in board.splitlines() if line.startswith("[")})
        raise Failure(f"看板上等不到期望的内容，读到的消息：{' | '.join(seen)[-1500:]}")

    def finish(self, text="目标已完成", timeout=60):
        self.fake.finish_lead(text)
        return self.wait_event("run.delivered", timeout)

    def pending(self, kind):
        """Requests of `kind` (approval.requested, input.requested, question.escalated) still open."""
        evs = self.events()
        done = {e.get("data", {}).get("key") for e in evs if e["type"] in (
            "approval.granted", "approval.denied", "input.answered", "question.answered", "decision.withdrawn")}
        return [e for e in evs if e["type"] == kind and e.get("data", {}).get("key") not in done]


def _match(e, k, v):
    if k.startswith("data__"):
        return (e.get("data") or {}).get(k[6:]) == v
    return e.get(k) == v


def read_file(path):
    with open(path) as f:
        return f.read()


def jsonl(text):
    return [json.loads(line) for line in text.splitlines() if line.strip()]


# The scenario modules register themselves on import, in this order.
from scenarios import setup, journeys, review, decisions, failures, controls, sessions, chats, roles, skills, adapters, worktrees, queue, seats, loops, memory, host_env, devices, restart, app_server, language, real  # noqa: E402,F401
