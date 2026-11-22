"""The product end-to-end scenarios and what they share (docs/operations/20261031-testing.md#product_e2e).

A scenario is a function `(ctx, c)` registered with `@scenario`: `ctx` is the running server (and
app), `c` records checks. Each one builds its own team (agents, digital employees, workspace and
a scripted agent directory) so scenarios run side by side on one server; the ones that restart
the server are `serial` and run after the rest.
"""

import json
import os
import threading
import time

from lib import ApiError, Failure, Fake, drawn, git_repo, http, wait


class Scenario:
    def __init__(self, fn, name, serial, first, real):
        self.fn, self.name, self.serial, self.first, self.real = fn, name, serial, first, real
        self.doc = (fn.__doc__ or "").strip().splitlines()[0]


SCENARIOS = []


def scenario(name, serial=False, first=False, real=False):
    """Register a scenario. `first` runs before all others (setup that others may rely on),
    `serial` after the parallel ones (it restarts or replaces the server), `real` only with --real."""
    def add(fn):
        SCENARIOS.append(Scenario(fn, name, serial, first, real))
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
from scenarios import setup, journeys, decisions, failures, controls, sessions, chats, skills, adapters, worktrees, queue, seats, loops, host_env, restart, app_server, real  # noqa: E402,F401
