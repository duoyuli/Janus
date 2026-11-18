#!/usr/bin/env python3
"""End-to-end data upgrade test: old releases write data, the current build upgrades it.

For each release tag (default: the three newest `v*` tags) this builds that release's server from
the tag's source, starts it on a scratch Janus home, writes data through its API the way the app
does (agents, default and custom digital employees, an edited default, a workspace) plus a
finished run with that release's member snapshot, stops it, and starts the current build on the
same home. Then it checks what the upgrade promises:

- the data version is the latest, a backup was taken, and `janus migrate --dry-run` finds every
  stored object in the latest shape;
- untouched default digital employees carry the current default text; an edited one keeps the
  person's text and offers the new one; custom data is unchanged apart from the migrated fields;
- a second start upgrades nothing and takes no new backup;
- a database from a newer Janus is refused.

Nothing in the checkout is touched: sources, builds and homes live in a temporary directory, and
the servers listen on 127.0.0.1:7979 (never the person's own 7878).

Usage: backend/scripts/upgrade_e2e.py [--from TAG ...] [--keep] [--dump-fixture PATH]
`--dump-fixture` writes the oldest tag's data (configs and runs, before the upgrade) as SQL for
`backend/testdata/upgrade/`.
"""

import argparse
import json
import os
import shutil
import signal
import sqlite3
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

ROOT = subprocess.run(["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True, check=True).stdout.strip()
ADDR = "127.0.0.1:7979"
FAILS = []


def say(msg):
    print(msg, flush=True)


def check(ok, what):
    say(("  ✓ " if ok else "  ✗ ") + what)
    if not ok:
        FAILS.append(what)


def run(cmd, **kw):
    return subprocess.run(cmd, check=True, **kw)


def seeds_in(source):
    """The default roles' id → (desc, prompt) in a version of roles.rs."""
    src = source[source.index("pub(super) const SEEDS") :]
    out = {}
    for blk in src.split("RoleSeed {")[1:]:
        f = {}
        for line in blk.split("\n"):
            t = line.strip()
            for k in ("id", "desc", "prompt"):
                if t.startswith(k + ': "'):
                    f[k] = json.loads(t[len(k) + 2 :].rstrip(","))
        out[f["id"]] = (f["desc"], f["prompt"])
    return out


def api(method, path, body=None):
    req = urllib.request.Request(
        f"http://{ADDR}{path}",
        data=None if body is None else json.dumps(body).encode(),
        headers={"Content-Type": "application/json"},
        method=method,
    )
    try:
        with urllib.request.urlopen(req, timeout=10) as r:
            return json.load(r)
    except urllib.error.HTTPError as e:
        raise RuntimeError(f"{method} {path}: {e.code} {e.read().decode()}") from None


class Server:
    def __init__(self, binary, home, log):
        self.binary, self.home, self.log = binary, home, log
        self.proc = None

    def start(self):
        env = dict(os.environ, JANUS_HOME=self.home, JANUS_ADDR=ADDR)
        self.proc = subprocess.Popen([self.binary], env=env, stdout=open(self.log, "a"), stderr=subprocess.STDOUT)
        for _ in range(150):
            if self.proc.poll() is not None:
                raise RuntimeError(f"{self.binary} exited ({self.proc.returncode}); see {self.log}")
            try:
                return api("GET", "/api/meta")
            except (OSError, RuntimeError):
                time.sleep(0.2)
        raise RuntimeError(f"{self.binary} did not answer on {ADDR}")

    def stop(self):
        if self.proc and self.proc.poll() is None:
            self.proc.send_signal(signal.SIGTERM)
            try:
                self.proc.wait(10)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait()


def build_old(tag, tmp):
    """The tag's server binary, built from the tag's own source in `tmp`."""
    src = os.path.join(tmp, "src-" + tag)
    os.makedirs(src)
    archive = subprocess.run(["git", "-C", ROOT, "archive", tag, "backend"], capture_output=True, check=True).stdout
    run(["tar", "-x", "-C", src], input=archive)
    # One target directory per tag: a shared one can leave the previous tag's binary in place.
    target = os.path.join(tmp, "target-" + tag)
    say(f"构建 {tag} 的服务…")
    run(["cargo", "build", "--locked", "-q", "--manifest-path", os.path.join(src, "backend", "Cargo.toml")],
        env=dict(os.environ, CARGO_TARGET_DIR=target))
    binary = os.path.join(tmp, "janus-" + tag)
    shutil.copy(os.path.join(target, "debug", "janus"), binary)
    old_roles = subprocess.run(["git", "-C", ROOT, "show", f"{tag}:backend/src/agents/roles.rs"], capture_output=True, text=True, check=True).stdout
    return binary, seeds_in(old_roles)


def old_member(id_, name, runtime, lead, role_id):
    """A run member as the 2026.11.18 releases wrote it: with the role label and the model label."""
    return {
        "id": id_, "name": name, "role": "旧定位", "desc": "旧职责", "runtime": runtime, "lead": lead, "color": "#5452e8",
        "init": "LD", "model": "", "effort": "", "session": {}, "permission": "Auto", "model_label": "Opus（Agent 默认）",
        "kind": "claude", "runtime_name": "Claude", "prompt": "", "role_prompt": "", "resources": [], "role_id": role_id,
    }


def write_old_data(ws):
    """Data written through the old release's API, the way the app writes it."""
    conn = api("POST", "/api/setup/connections", {"kind": "claude", "name": "Claude", "auth": "apikey", "apiKey": "sk-ant-e2e"})
    custom = api("POST", "/api/setup/connections", {"kind": "custom", "name": "Fake", "command": "python3 fake_acp.py"})
    setup = api("GET", "/api/setup")
    roles = {r["id"]: r for r in setup["roles"]}
    for rid in ("coordinator", "implementer", "reviewer"):
        body = dict(roles[rid], connection=conn["id"])
        if rid == "coordinator":
            body["lead"] = True
        if rid == "reviewer":
            body["prompt"] = "我自己写的质量要求：先跑全部测试。"
        api("POST", "/api/setup/roles", body)
    tester = api("POST", "/api/setup/roles", {"name": "测试员", "role": "旧定位", "desc": "写测试", "connection": custom["id"], "prompt": "只写测试"})
    api("POST", "/api/setup/workspaces", {
        "name": "e2e", "path": ws, "lead": "coordinator", "brief": "共享资料",
        "members": [{"role": "coordinator"}, {"role": "implementer"}, {"role": "reviewer"}, {"role": tester["id"], "slots": 2}],
    })
    return conn, custom, tester


def insert_old_run(home):
    team = [old_member("lead", "PMO", "claude", True, "coordinator"), old_member("implementer", "研发工程师", "claude", False, "implementer")]
    db = sqlite3.connect(os.path.join(home, "janus.db"))
    db.execute(
        "INSERT INTO runs (id, goal, cwd, config, status, created_at) VALUES ('run-old', '旧目标', '/tmp', ?, 'delivered', 1)",
        (json.dumps({"team": team, "git": False, "baseRef": None, "workspace": "e2e", "goalImages": []}),),
    )
    db.commit()
    db.close()


def dump_fixture(home, ws, path):
    db = sqlite3.connect(os.path.join(home, "janus.db"))
    lines = ["-- Data written by an old release (data version 0), for the upgrade tests. Made by",
             "-- backend/scripts/upgrade_e2e.py --dump-fixture; paths point nowhere on purpose."]
    for kind, id_, data in db.execute("SELECT kind, id, data FROM configs ORDER BY kind, id"):
        for real, fake in ((home, "/tmp/janus-fixture/home"), (os.path.realpath(ws), "/tmp/janus-fixture/ws"), (ws, "/tmp/janus-fixture/ws")):
            data = data.replace(real, fake)
        lines.append("INSERT INTO configs (kind, id, data, updated_at) VALUES ({}, {}, {}, 1);".format(*(sql_text(x) for x in (kind, id_, data))))
    for id_, goal, cwd, config, status in db.execute("SELECT id, goal, cwd, config, status FROM runs ORDER BY id"):
        lines.append("INSERT INTO runs (id, goal, cwd, config, status, created_at) VALUES ({}, {}, {}, {}, {}, 1);".format(*(sql_text(x) for x in (id_, goal, cwd, config, status))))
    db.close()
    with open(path, "w") as f:
        f.write("\n".join(lines) + "\n")
    say(f"已写出样本数据：{path}")


def sql_text(s):
    return "'" + str(s).replace("'", "''") + "'"


def db_version(home):
    db = sqlite3.connect(os.path.join(home, "janus.db"))
    v = db.execute("PRAGMA user_version").fetchone()[0]
    db.close()
    return v


def backups(home):
    d = os.path.join(home, "backups")
    return sorted(os.listdir(d)) if os.path.isdir(d) else []


def upgrade_from(tag, tmp, new_bin, latest, current_seeds, dump):
    say(f"\n== 从 {tag} 升级")
    old_bin, old_seeds = build_old(tag, tmp)
    home, ws = os.path.join(tmp, "home-" + tag), os.path.join(tmp, "ws-" + tag)
    os.makedirs(ws)
    run(["git", "init", "-q", ws])
    log = os.path.join(tmp, f"janus-{tag}.log")
    old = Server(old_bin, home, log)
    old.start()
    try:
        conn, custom, tester = write_old_data(ws)
        before = api("GET", "/api/setup")
    finally:
        old.stop()
    insert_old_run(home)
    if dump:
        dump_fixture(home, ws, dump)

    env = dict(os.environ, JANUS_HOME=home)
    preview = subprocess.run([new_bin, "migrate", "--dry-run"], env=env, capture_output=True, text=True)
    check(preview.returncode == 0 and f"→ {latest}" in preview.stdout and "所有数据都符合" in preview.stdout,
          f"migrate --dry-run 预览升级到 {latest} 且没有遗留问题：{preview.stdout.strip() or preview.stderr.strip()}")
    check(db_version(home) == 0 and not backups(home), "预览没有改动数据")

    new = Server(new_bin, home, log)
    meta = new.start()
    try:
        check(meta.get("dataVersion") == latest, f"数据版本升到 {latest}（实际 {meta.get('dataVersion')}）")
        check(len(backups(home)) == 1 and backups(home)[0].startswith("janus-v0-"), f"升级前做了备份：{backups(home)}")
        setup = api("GET", "/api/setup")
        roles = {r["id"]: r for r in setup["roles"]}
        check(all("role" not in r for r in setup["roles"]), "数字员工不再带「定位」字段")
        for rid in ("coordinator", "researcher", "product-manager", "implementer"):
            r = roles[rid]
            check((r["desc"], r["prompt"]) == current_seeds[rid] and "seedUpdates" not in r,
                  f"没改过的默认数字员工 {rid} 换成当前默认文本" + ("" if old_seeds[rid] != current_seeds[rid] else "（这个版本的默认文本没变）"))
            check(r.get("seed", {}).get("id") == rid, f"{rid} 记录了来源")
        rv = roles["reviewer"]
        check(rv["prompt"] == "我自己写的质量要求：先跑全部测试。", "改过指令的 reviewer 保留用户的文本")
        check(rv["desc"] == current_seeds["reviewer"][0], "reviewer 没改过的职责换成当前默认文本")
        check(rv.get("seedUpdates", {}).get("prompt") == current_seeds["reviewer"][1], "reviewer 提示有新版默认指令")
        t = roles[tester["id"]]
        b = {r["id"]: r for r in before["roles"]}[tester["id"]]
        check({k: v for k, v in t.items() if k != "role"} == {k: v for k, v in b.items() if k != "role"}, "自定义数字员工除「定位」外不变")
        conns = {c["id"]: c for c in setup["connections"]}
        check(conns[conn["id"]]["name"] == "Claude" and conns[custom["id"]]["command"] == "python3 fake_acp.py", "Agent 连接不变")
        check(setup["workspaces"] == before["workspaces"], "工作区不变")
        db = sqlite3.connect(os.path.join(home, "janus.db"))
        team = json.loads(db.execute("SELECT config FROM runs WHERE id = 'run-old'").fetchone()[0])["team"]
        db.close()
        check(all("role" not in m and "model_label" not in m for m in team) and team[0]["role_id"] == "coordinator",
              "Run 快照的成员去掉了定位和模型标签，其余不变")
        kept = api("POST", "/api/roles/reviewer/seed", {"field": "prompt", "adopt": False})
        check(kept["prompt"] == rv["prompt"] and "seedUpdates" not in {r["id"]: r for r in api("GET", "/api/setup")["roles"]}["reviewer"],
              "选择「保留我的」后不再提示")
    finally:
        new.stop()

    after = subprocess.run([new_bin, "migrate", "--dry-run"], env=env, capture_output=True, text=True)
    check("已是最新" in after.stdout and "所有数据都符合" in after.stdout, f"升级后 migrate --dry-run 没有遗留问题：{after.stdout.strip()}")
    meta = new.start()
    new.stop()
    check(meta.get("dataVersion") == latest and len(backups(home)) == 1, "再次启动不重复升级、不再备份")

    newer = os.path.join(tmp, "home-newer-" + tag)
    shutil.copytree(home, newer, ignore=shutil.ignore_patterns("janus.pid"))
    db = sqlite3.connect(os.path.join(newer, "janus.db"))
    db.execute(f"PRAGMA user_version = {latest + 1}")
    db.commit()
    db.close()
    refused = subprocess.run([new_bin], env=dict(os.environ, JANUS_HOME=newer, JANUS_ADDR=ADDR), capture_output=True, text=True, timeout=30)
    check(refused.returncode != 0 and "更新的 Janus" in (refused.stdout + refused.stderr), "拒绝打开更新版本的数据")


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--from", dest="tags", action="append", help="release tag to upgrade from (repeatable)")
    p.add_argument("--keep", action="store_true", help="keep the temporary directory")
    p.add_argument("--dump-fixture", help="write the oldest tag's pre-upgrade data as SQL to this path")
    a = p.parse_args()
    tags = a.tags or subprocess.run(["git", "-C", ROOT, "tag", "--list", "v*", "--sort=-creatordate"], capture_output=True, text=True, check=True).stdout.split()[:3]
    tmp = tempfile.mkdtemp(prefix="janus-upgrade-e2e-")
    say(f"临时目录：{tmp}")
    try:
        say("构建当前版本的服务…")
        run(["cargo", "build", "--locked", "-q"], cwd=os.path.join(ROOT, "backend"))
        new_bin = os.path.join(ROOT, "backend", "target", "debug", "janus")
        shape = json.loads(subprocess.run([new_bin, "migrate", "--shape"], capture_output=True, text=True, check=True).stdout)
        with open(os.path.join(ROOT, "backend", "src", "agents", "roles.rs")) as f:
            current_seeds = seeds_in(f.read())
        oldest = sorted(tags)[0]
        for tag in tags:
            upgrade_from(tag, tmp, new_bin, shape["dataVersion"], current_seeds, a.dump_fixture if tag == oldest else None)
    finally:
        if a.keep:
            say(f"保留了临时目录：{tmp}")
        else:
            shutil.rmtree(tmp, ignore_errors=True)
    say("\n升级端到端测试：" + ("全部通过" if not FAILS else f"{len(FAILS)} 项失败"))
    for f in FAILS:
        say("  ✗ " + f)
    sys.exit(1 if FAILS else 0)


if __name__ == "__main__":
    main()
