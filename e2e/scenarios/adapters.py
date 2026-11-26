"""Runtime updates: Janus checks for a newer version itself shortly after it starts, installs it in
the background and keeps the previous one for switching back; picking a version pins it until the
person resumes automatic updates (docs/architecture/20261031-adapters.md#update_check). The app is
left on 「设置 › 数字员工」 the whole time: every state below is what the open page read by itself,
never a `reload` that would hide a page that does not refresh."""

import os
import shutil

from lib import ADAPTER_PACKAGES, STAND_IN_VERSION, Registry, drawn, wait
from scenarios import read_file, scenario

CLAUDE = ADAPTER_PACKAGES["claude"]
NEW = "9.10.0"


@scenario("adapter-updates", own=True)
def adapter_updates(ctx, c):
    """运行组件自动升级：服务启动后自己发现并安装新版本，旧版本保留；设置页开着时自己刷新；切回旧版本即固定，重启服务后仍固定；恢复自动更新回到最新"""
    reg = Registry()
    reg.latest[CLAUDE] = NEW
    with ctx.private("adapters", npm_config_registry=reg.url) as ctx:
        def claude():
            return next(a for a in ctx.api("GET", "/api/adapters")["adapters"] if a["id"] == "claude")

        # One Agent of each kind, so the settings page has both cards with their runtime lines.
        # Saving an Agent does not start its check, so nothing else touches the adapters. This
        # happens before the server's own check, so the page below can stay open while the update
        # arrives: the app never asks for it.
        fake = ctx.path("adapters", "fake")
        ctx.api("POST", "/api/setup/connections", {
            "id": "claude-1", "kind": "claude", "name": "Claude", "auth": "apikey", "envText": f"FAKE_DIR={fake}",
        })
        ctx.api("POST", "/api/setup/connections", {
            "id": "codex-1", "kind": "codex", "name": "Codex", "auth": "apikey",
            "apiKey": "sk-e2e-not-a-key", "envText": f"FAKE_DIR={fake}",
        })

        if ctx.app:
            # The new Agents are read once (the app was already running when they were created),
            # then the page stays open. From here on the app is only asked for `state` until the
            # automatic update has shown up: no `reload` may hide a page that does not refresh.
            st = ctx.app.until(
                lambda s: any(x["id"] == "claude" for x in s.get("adapters", [])),
                "设置 › 数字员工显示运行组件一行", "reload", "open settings:agents", timeout=30)
            row = next(x for x in st["adapters"] if x["id"] == "claude")
            c(row["current"] == STAND_IN_VERSION and row["state"] == "idle",
              f"应用：自动升级前显示 {STAND_IN_VERSION}，没有状态词（{row['current']}/{row['state']}）")
            c(row["menuShown"] is False, "应用：只有一个已安装版本时没有切回入口")

            # The server checks 45 seconds after start and installs by itself. The app is told
            # nothing: the new version can only show because the open page reads again.
            st = ctx.app.until(
                lambda s: any(x["id"] == "claude" and x["current"] == NEW and STAND_IN_VERSION in x["versions"]
                              for x in s.get("adapters", [])),
                "设置页开着时自己显示自动升级到的新版本（没有 reload）", timeout=180)
            c(st["settings"] is True and st["settingsTab"] == "agents", "等待期间设置 › 数字员工页一直开着")

        # Nobody asks for anything: the server checks on its own 45 seconds after start and
        # installs what it finds. This is the automatic path, through a real `npm install`.
        def upgraded():
            a = claude()
            return a if a["current"] == NEW else None

        a = wait(upgraded, f"服务自动把 claude 升到 {NEW}（不需要手动触发）", 60)
        c(not a["pinned"], "自动升级后不是固定状态")
        c(any(v["version"] == STAND_IN_VERSION for v in a["versions"]), f"旧版本 {STAND_IN_VERSION} 保留，可以切回")
        c((a["job"] or {}).get("action") == "auto" and a["job"]["state"] == "ok",
          f"这次升级记为自动任务（{a['job']['action']}/{a['job']['state']}）")
        c(a["latest"] == NEW and not a["updateAvailable"], "升级后已是最新，没有可升级项")
        c(CLAUDE in reg.asked, "服务自己向 registry 查询了 claude 的最新版本")

        if ctx.app:
            st = ctx.app.do()
            claude_row = next(x for x in st["adapters"] if x["id"] == "claude")
            c(claude_row["state"] == "autoUpdated", f"应用：刚自动更新完显示「已自动更新」（{claude_row['state']}）")
            c(claude_row["menu"] == [f"switch:{STAND_IN_VERSION}"], f"应用：菜单里是切回 {STAND_IN_VERSION}（{claude_row['menu']}）")
            codex_row = next(x for x in st["adapters"] if x["id"] == "codex")
            c(codex_row["menuShown"] is False, "应用：只有一个已装版本的运行组件没有切回入口")
            ctx.app.do("shot adapter-line")
            c(drawn(ctx.app.picture("adapter-line")), "应用：设置 › 数字员工的运行组件一行画出来了（adapter-line.png）")
            # The reminder that used to sit in the sidebar is gone with the automatic update.
            c("adapterUpdates" not in ctx.app.do("open chats"),
              "应用状态里不再有侧栏的适配器更新提醒（adapterUpdates）")
            ctx.app.do("open settings:agents")

        # The menu's 「切回」 does the same thing as this call: switch and pin. The page is open,
        # so the pinned state has to arrive through its own read (no reload).
        ctx.api("POST", "/api/adapters/claude/use", {"version": STAND_IN_VERSION})
        a = claude()
        c((a["current"], a["pinned"], a["latest"]) == (STAND_IN_VERSION, True, NEW),
          f"切回 {STAND_IN_VERSION} 后固定，仍知道最新是 {NEW}（{a['current']}/{a['pinned']}/{a['latest']}）")
        pin = os.path.join(ctx.home, "cache", "adapters", "claude", "pinned")
        c(os.path.exists(pin) and read_file(pin).strip() == STAND_IN_VERSION,
          "固定的版本号记在 cache/adapters/claude/pinned 里")

        if ctx.app:
            st = ctx.app.until(
                lambda s: any(x["id"] == "claude" and x["current"] == STAND_IN_VERSION and x["pinned"]
                              for x in s.get("adapters", [])),
                "应用在设置页开着时显示切回后的固定版本（没有 reload）", timeout=60)
            claude_row = next(x for x in st["adapters"] if x["id"] == "claude")
            c(claude_row["state"] == "idle" and claude_row["menuShown"] is True,
              f"应用：固定后没有状态词，菜单里可以恢复自动更新（{claude_row['state']}/{claude_row['menu']}）")
            c(claude_row["menu"] == [f"resume:{NEW}", f"switch:{NEW}"],
              f"应用：固定时菜单首项是恢复自动更新（{claude_row['menu']}）")
            ctx.app.do("shot adapter-pinned")
            c(drawn(ctx.app.picture("adapter-pinned")), "应用：固定状态画出来了（adapter-pinned.png）")

        # Pinned state is on disk, not in the process: a restart keeps it, and the open page
        # reads it back by itself.
        ctx.server.stop()
        ctx.server.start()
        a = wait(lambda: (lambda x: x if (x["current"], x["pinned"]) == (STAND_IN_VERSION, True) else None)(claude()),
                 "重启服务后仍是切回的版本，仍然固定", 30)
        c(True, "重启服务后仍是切回的版本，仍然固定")
        if ctx.app:
            ctx.app.until(
                lambda s: any(x["id"] == "claude" and x["current"] == STAND_IN_VERSION and x["pinned"]
                              for x in s.get("adapters", [])),
                "应用在服务重启后仍显示固定版本（没有 reload）", timeout=90)
            c(True, "应用：服务重启后固定版本仍然显示")

        # 恢复自动更新: unpin and go back to the newest version, already on disk.
        ctx.api("POST", "/api/adapters/claude/install", {})

        def resumed():
            x = claude()
            return x if x["current"] == NEW and not x["pinned"] and (x["job"] or {}).get("state") == "ok" else None

        a = wait(resumed, "恢复自动更新后回到最新的已安装版本", 60)
        c(a["job"]["action"] == "install", "恢复自动更新是一次手动安装任务")
        c(not os.path.exists(pin), "恢复后不再固定")
        c(any(v["version"] == STAND_IN_VERSION for v in a["versions"]), "旧版本仍保留，随时可以再切回")
        c(a["latest"] == NEW and not a["updateAvailable"], "恢复后已是最新")
        if ctx.app:
            st = ctx.app.until(
                lambda s: any(x["id"] == "claude" and x["current"] == NEW and not x["pinned"]
                              for x in s.get("adapters", [])),
                "应用在设置页开着时显示恢复自动更新后的版本（没有 reload）", timeout=60)
            c(next(x for x in st["adapters"] if x["id"] == "claude")["state"] == "idle",
              "应用：手动恢复自动更新后没有状态词")

        # An Agent whose runtime is not installed: the line says so, with a light 安装 link. The
        # server checks only after 45 seconds, so removing it now leaves it missing for the app.
        os.remove(os.path.join(ctx.home, "cache", "adapters", "codex", "current"))
        shutil.rmtree(os.path.join(ctx.home, "cache", "adapters", "codex", STAND_IN_VERSION))
        a = wait(lambda: (lambda x: x if not x["current"] and not x["versions"] else None)(
            next(x for x in ctx.api("GET", "/api/adapters")["adapters"] if x["id"] == "codex")),
            "服务把没有安装的运行组件报成未安装", 30)
        c(a["pinned"] is False and a["job"] is None, "未安装的运行组件没有固定状态，也没有遗留任务")
        if ctx.app:
            # Files removed behind the service's back send no notice: the page shows it when it
            # opens again, as after any change made outside Janus.
            st = ctx.app.until(
                lambda s: any(x["id"] == "codex" and not x["current"] and not x["versions"] for x in s.get("adapters", [])),
                "应用重新打开设置的数字员工页时显示未安装的运行组件", "open settings:general", "open settings:agents", timeout=60)
            row = next(x for x in st["adapters"] if x["id"] == "codex")
            c(row["state"] == "notInstalled" and row["menuShown"] is False,
              f"应用：未安装时只写「运行组件还没装」，没有切换入口（{row['state']}/{row['menuShown']}）")
            ctx.app.do("shot adapter-missing")
            c(drawn(ctx.app.picture("adapter-missing")), "应用：未安装状态画出来了（adapter-missing.png）")

        # A failed install shows only Janus's own wording. Stopping the registry makes the real
        # `npm install` fail; the popover would render the reason and the recent notes, which the
        # app state carries, and none of them may name npm, the package or `node_modules`.
        reg.close()
        ctx.api("POST", "/api/adapters/claude/install", {"version": "9.11.0"})
        failed = wait(lambda: (lambda x: x if (x["job"] or {}).get("state") == "error" else None)(claude()),
                      "安装失败记在 job 上", 90)
        detail = failed["job"]["detail"]
        c(not any(bad in detail.lower() for bad in ("npm", "@agentclientprotocol", "node_modules", "$ ")),
          f"失败原因只有 Janus 的措辞，不出现 npm、包名或命令行（{detail}）")
        c(failed["current"] == NEW, "升级失败不影响当前版本")
        raw = read_file(os.path.join(ctx.home, "logs", "adapters", "claude.log"))
        c("npm" in raw.lower(), "原始日志里仍有安装工具的输出，供「查看日志」打开")
        if ctx.app:
            st = ctx.app.until(
                lambda s: any(x["id"] == "claude" and x["state"] == "failed" for x in s.get("adapters", [])),
                "应用在设置页开着时显示「更新没成功」", timeout=60)
            row = next(x for x in st["adapters"] if x["id"] == "claude")
            shown = [row.get("reason") or ""] + list(row.get("recent") or [])
            c(not any(bad in text.lower() for text in shown
                      for bad in ("npm", "@agentclientprotocol", "node_modules", "$ ")),
              f"失败弹层显示的原因和最近日志不含 npm、包名或命令行（{shown}）")

        # 装的比 npm 上的还新时不算新版本 is decided by the same comparison the backend test
        # covers (`only_a_later_version_is_an_update`); a check here would wait 6 hours.
