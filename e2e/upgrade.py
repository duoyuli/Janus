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
the servers listen on a free port of 127.0.0.1 (never the person's own 7878).

Usage: e2e/upgrade.py [--from TAG ...] [--keep] [--dump-fixture PATH]
`--dump-fixture` writes the oldest tag's data (configs and runs, before the upgrade) as SQL for
`backend/testdata/upgrade/`.
"""

import argparse
import json
import os
import shutil
import sqlite3
import subprocess
import sys
import tempfile

# Imports from e2e/ must not leave __pycache__ in the checkout (AGENTS.md rule 9).
sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from lib import ADDR, ROOT, ApiError, Checks, Server, foreign, home_db, http, run, say  # noqa: E402

check = Checks("升级")


# Default roles whose id changed in data version 17 (`RENAMED` in backend/src/agents/roles.rs).
RENAMED = {"coordinator": "pmo", "researcher": "marketing", "implementer": "developer", "reviewer": "qa", "inner-grill": "friend"}


def seeds_in(backend):
    """The default roles' id → (desc, prompt) in a version's `backend/`: one Markdown file each in
    `roles/` (metadata between `---` lines, then the instructions), or, in releases before that,
    the `SEEDS` of `src/agents/roles.rs`."""
    roles = os.path.join(backend, "roles")
    if os.path.isdir(roles):
        out = {}
        for name in sorted(os.listdir(roles)):
            if not name.endswith(".md"):
                continue
            with open(os.path.join(roles, name)) as f:
                head, body = f.read()[len("---\n"):].split("\n---\n", 1)
            meta = dict(line.split(":", 1) for line in head.splitlines())
            out[meta["id"].strip()] = (meta["desc"].strip().strip('"'), body.strip())
        return out
    with open(os.path.join(backend, "src", "agents", "roles.rs")) as f:
        source = f.read()
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
    return http(ADDR, method, path, body)


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
    return binary, seeds_in(os.path.join(src, "backend"))


def old_member(id_, name, runtime, lead, role_id, version):
    """A run member as a release at data `version` wrote it. Data version 0 still had the role
    label and the model label; a later migration that changes members adds its shape here."""
    m = {
        "id": id_, "name": name, "desc": "旧职责", "runtime": runtime, "lead": lead, "color": "#5452e8",
        "init": "LD", "model": "", "effort": "", "session": {}, "permission": "Auto",
        "kind": "claude", "runtime_name": "Claude", "prompt": "", "role_prompt": "", "resources": [], "role_id": role_id,
    }
    if version == 0:
        m.update({"role": "旧定位", "model_label": "Opus（Agent 默认）"})
    return m


def write_old_data(ws):
    """Data written through the old release's API, the way the app writes it."""
    # Before 2026.11.21 the key had a field of its own; from then on it is a line of the environment.
    conn = api("POST", "/api/setup/connections", {"kind": "claude", "name": "Claude", "auth": "apikey", "apiKey": "sk-ant-e2e",
                                                  "envText": "ANTHROPIC_API_KEY=sk-ant-e2e"})
    try:
        custom = api("POST", "/api/setup/connections", {"kind": "custom", "name": "Fake", "command": "python3 fake_acp.py"})
    except ApiError:
        custom = None  # releases from 2026.11.20 on run only Codex and Claude Code
    setup = api("GET", "/api/setup")
    roles = {r["id"]: r for r in setup["roles"]}
    for rid in ("coordinator", "implementer", "reviewer"):
        body = dict(roles[rid], connection=conn["id"])
        if rid == "coordinator":
            body["lead"] = True
        if rid == "reviewer":
            body["prompt"] = "我自己写的质量要求：先跑全部测试。"
        api("POST", "/api/setup/roles", body)
    tester = api("POST", "/api/setup/roles", {"name": "测试员", "role": "旧定位", "desc": "写测试", "connection": (custom or conn)["id"], "prompt": "只写测试"})
    api("POST", "/api/setup/workspaces", {
        "name": "e2e", "path": ws, "lead": "coordinator", "brief": "共享资料",
        "members": [{"role": "coordinator"}, {"role": "implementer"}, {"role": "reviewer"}, {"role": tester["id"], "slots": 2}],
    })
    return conn, custom, tester


def insert_old_run(home, version, ws):
    """A delivered run working in place, and one in a worktree of `ws`. Written in the shape of
    data version `version`: before version 10 a worktree has no record of the branch it started on."""
    team = [old_member("lead", "PMO", "claude", True, "coordinator", version),
            old_member("implementer", "研发工程师", "claude", False, "implementer", version)]
    head = subprocess.run(["git", "-C", ws, "rev-parse", "HEAD"], capture_output=True, text=True, check=True).stdout.strip()
    worktree = {"repo": ws}
    if version >= 10:
        worktree["branch"] = subprocess.run(["git", "-C", ws, "branch", "--show-current"], capture_output=True, text=True,
                                            check=True).stdout.strip()
    db = sqlite3.connect(home_db(home))
    db.execute(
        "INSERT INTO runs (id, goal, cwd, config, status, created_at) VALUES ('run-old', '旧目标', '/tmp', ?, 'delivered', 1)",
        (json.dumps({"team": team, "git": False, "baseRef": None, "workspace": "e2e", "goalImages": []}),),
    )
    db.execute(
        "INSERT INTO runs (id, goal, cwd, config, status, created_at) VALUES ('run-wt', '旧目标', '/tmp/wt', ?, 'delivered', 2)",
        (json.dumps({"team": team, "git": True, "baseRef": head, "workspace": "e2e", "goalImages": [], "worktree": worktree}),),
    )
    db.commit()
    db.close()


def dump_fixture(home, ws, path):
    db = sqlite3.connect(home_db(home))
    lines = ["-- Data written by an old release (data version 0), for the upgrade tests. Made by",
             "-- e2e/upgrade.py --dump-fixture; paths point nowhere on purpose."]
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
    db = sqlite3.connect(home_db(home))
    v = db.execute("PRAGMA user_version").fetchone()[0]
    db.close()
    return v


def backups(home):
    d = os.path.join(home, "backups")
    return sorted(os.listdir(d)) if os.path.isdir(d) else []


def upgrade_from(tag, tmp, new_bin, latest, current_seeds, dump):
    say(f"\n== 从 {tag} 升级")
    old_bin, old_seeds = build_old(tag, tmp)
    # Compared by the ids the roles have after upgrading.
    old_seeds = {RENAMED.get(k, k): v for k, v in old_seeds.items()}
    home, ws = os.path.join(tmp, "home-" + tag), os.path.join(tmp, "ws-" + tag)
    os.makedirs(ws)
    run(["git", "init", "-q", ws])
    run(["git", "-C", ws, "-c", "user.name=e2e", "-c", "user.email=e2e@e2e", "commit", "-q", "--allow-empty", "-m", "init"])
    log = os.path.join(tmp, f"janus-{tag}.log")
    old = Server(old_bin, home, log)
    old.start()
    try:
        conn, custom, tester = write_old_data(ws)
        before = api("GET", "/api/setup")
    finally:
        old.stop()
    old_version = db_version(home)
    insert_old_run(home, old_version, ws)
    upgrades = old_version < latest
    say(f"  {tag} 的数据版本是 {old_version}" + ("" if upgrades else "，已是最新，没有要升级的"))
    if dump:
        dump_fixture(home, ws, dump)

    env = dict(os.environ, JANUS_HOME=home)
    preview = subprocess.run([new_bin, "migrate", "--dry-run"], env=env, capture_output=True, text=True)
    expect = f"数据版本 {old_version} → {latest}" if upgrades else "已是最新"
    check(preview.returncode == 0 and expect in preview.stdout and "所有数据都符合" in preview.stdout,
          f"migrate --dry-run 预览（{expect}）且没有遗留问题：{preview.stdout.strip() or preview.stderr.strip()}")
    check(db_version(home) == old_version and not backups(home), "预览没有改动数据")

    new = Server(new_bin, home, log)
    meta = new.start()
    try:
        check(meta.get("dataVersion") == latest, f"数据版本升到 {latest}（实际 {meta.get('dataVersion')}）")
        if upgrades:
            check(len(backups(home)) == 1 and backups(home)[0].startswith(f"janus-v{old_version}-"), f"升级前做了备份：{backups(home)}")
        else:
            check(not backups(home), "没有要升级的数据时不备份")
        setup = api("GET", "/api/setup")
        claude = next(c for c in setup["connections"] if c["id"] == "claude")
        check(claude["auth"] == "apikey" and "ANTHROPIC_API_KEY" in claude["env"] and claude["env"]["ANTHROPIC_API_KEY"] is None,
              "旧 Claude API Key 升级为已保存的私有环境变量")
        with open(os.path.join(home, "data", "secrets", "agents", "claude.json")) as f:
            check(json.load(f)["apiKey"] == "sk-ant-e2e", "升级保留原来的 Claude 密钥，移到 data/secrets/")
        check(not foreign(home) and os.path.exists(os.path.join(home, "data", "janus.db")),
              f"Janus home 按类别整理：顶层只有布局声明过的名字（{sorted(os.listdir(home))}）")
        check(os.path.isdir(os.path.join(home, "data", "agents", "claude", "config")), "Agent 的配置目录移到 data/agents/<id>/config/")
        roles = {r["id"]: r for r in setup["roles"]}
        check(all("role" not in r for r in setup["roles"]), "数字员工不再带「定位」字段")
        for then in ("coordinator", "researcher", "implementer"):
            check(then not in roles, f"默认数字员工 {then} 改用职能命名的 id {RENAMED[then]}")
        for rid in ("pmo", "marketing", "product-manager", "developer"):
            r = roles[rid]
            check((r["desc"], r["prompt"]) == current_seeds[rid] and "seedUpdates" not in r,
                  f"没改过的默认数字员工 {rid} 换成当前默认文本" + ("" if old_seeds[rid] != current_seeds[rid] else "（这个版本的默认文本没变）"))
            check(r.get("seed", {}).get("id") == rid, f"{rid} 记录了来源")
        friend = roles.get("friend") or {}
        check(friend.get("name") == "老友" and friend.get("seed", {}).get("id") == "friend"
              and (friend.get("desc"), friend.get("prompt")) == current_seeds["friend"], "升级后有默认的数字员工「老友」，文本是当前版本的")
        check(all(roles[r].get("avatar") for r in current_seeds), "升级后每位默认数字员工都有默认头像")
        rv = roles["qa"]
        check(rv["prompt"] == "我自己写的质量要求：先跑全部测试。", "改过指令的 reviewer 保留用户的文本")
        check(rv["desc"] == current_seeds["qa"][0], "reviewer 没改过的职责换成当前默认文本")
        # Data from before data version 2 does not say which default an edited field started from,
        # so the current default is offered once; later, only a changed default is offered.
        if old_seeds["qa"][1] != current_seeds["qa"][1] or old_version < 2:
            check(rv.get("seedUpdates", {}).get("prompt") == current_seeds["qa"][1], "reviewer 提示有新版默认指令")
        else:
            check("seedUpdates" not in rv, "默认指令没有新版本时 reviewer 不提示")
        t = roles[tester["id"]]
        b = {r["id"]: r for r in before["roles"]}[tester["id"]]
        # Version 1 dropped `role`, version 9 may unbind a custom-command agent, version 11 dropped
        # `resources`, version 16 added `avatar` (empty: no picture).
        changed = ("role", "connection", "resources", "avatar")
        check({k: v for k, v in t.items() if k not in changed} == {k: v for k, v in b.items() if k not in changed},
              "自定义数字员工除「定位」、选的资源和绑定的 Agent 外不变")
        check("resources" not in t and t.get("avatar") == "", "数字员工不再带选的资源，没有头像（显示文字）")
        conns = {c["id"]: c for c in setup["connections"]}
        if custom:
            check(t["connection"] == "", "绑定「自定义命令」Agent 的数字员工改为没有绑定 Agent")
            check(conns[conn["id"]]["name"] == "Claude" and custom["id"] not in conns, "Claude Code 连接不变，「自定义命令」的 Agent 已删除")
        else:
            check(t["connection"] == conn["id"] and conns[conn["id"]]["name"] == "Claude", "数字员工和绑定的 Claude Code 连接不变")
        renamed = json.loads(json.dumps(before["workspaces"]))
        for w in renamed:
            w["lead"] = RENAMED.get(w["lead"], w["lead"])
            for m in w["members"]:
                m["role"] = RENAMED.get(m["role"], m["role"])
        check(setup["workspaces"] == renamed, "工作区除了默认数字员工的新 id 外不变")
        db = sqlite3.connect(home_db(home))
        team = json.loads(db.execute("SELECT config FROM runs WHERE id = 'run-old'").fetchone()[0])["team"]
        worktree = json.loads(db.execute("SELECT config FROM runs WHERE id = 'run-wt'").fetchone()[0])["worktree"]
        db.close()
        branch = subprocess.run(["git", "-C", ws, "branch", "--show-current"], capture_output=True, text=True).stdout.strip()
        check(worktree == {"repo": ws, "branch": branch}, f"worktree 中的旧 Run 记下验收时合并回的分支 {branch}：{worktree}")
        check(all("role" not in m and "model_label" not in m for m in team) and team[0]["role_id"] == "pmo",
              "Run 快照的成员去掉了定位和模型标签，其余不变")
        kept = api("POST", "/api/roles/qa/seed", {"field": "prompt", "adopt": False})
        check(kept["prompt"] == rv["prompt"] and "seedUpdates" not in {r["id"]: r for r in api("GET", "/api/setup")["roles"]}["qa"],
              "选择「保留我的」后不再提示")
    finally:
        new.stop()

    after = subprocess.run([new_bin, "migrate", "--dry-run"], env=env, capture_output=True, text=True)
    check("已是最新" in after.stdout and "所有数据都符合" in after.stdout, f"升级后 migrate --dry-run 没有遗留问题：{after.stdout.strip()}")
    meta = new.start()
    new.stop()
    check(meta.get("dataVersion") == latest and len(backups(home)) == (1 if upgrades else 0), "再次启动不重复升级、不再备份")

    newer = os.path.join(tmp, "home-newer-" + tag)
    shutil.copytree(home, newer, ignore=shutil.ignore_patterns("janus.pid"))
    db = sqlite3.connect(home_db(newer))
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
        current_seeds = seeds_in(os.path.join(ROOT, "backend"))
        oldest = sorted(tags)[0]
        for tag in tags:
            upgrade_from(tag, tmp, new_bin, shape["dataVersion"], current_seeds, a.dump_fixture if tag == oldest else None)
    finally:
        if a.keep:
            say(f"保留了临时目录：{tmp}")
        else:
            shutil.rmtree(tmp, ignore_errors=True)
    failed = [what for ok, what in check.items if not ok]
    say("\n升级端到端测试：" + ("全部通过" if not failed else f"{len(failed)} 项失败"))
    for f in failed:
        say("  ✗ " + f)
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
