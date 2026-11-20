"""ACP adapter updates: Janus checks npm for newer adapters and reminds the person
(docs/architecture/20261031-adapters.md#update_check)."""

from lib import ADAPTER_PACKAGES, STAND_IN_VERSION, drawn, wait
from scenarios import scenario


@scenario("adapter-updates")
def adapter_updates(ctx, c):
    """ACP 适配器的新版本：服务启动后自动检查已安装的适配器，npm 上有新版本时应用在侧栏提醒，装的已是最新时不提醒"""
    reg = ctx.registry
    wait(lambda: set(ADAPTER_PACKAGES.values()) <= set(reg.asked), "服务自动检查适配器的新版本", 120)
    c(True, "服务启动后自动向 npm 检查了每个已安装的适配器")

    def adapter(id_):
        return next(a for a in ctx.api("GET", "/api/adapters")["adapters"] if a["id"] == id_)

    claude = adapter("claude")
    c(claude["latest"] == STAND_IN_VERSION and not claude["updateAvailable"] and not claude.get("job"),
      f"装的已是最新时不提醒，也不显示成适配器的任务（{claude['current']} / {claude['latest']}）")

    def check(id_):
        ctx.api("POST", f"/api/adapters/{id_}/check", {})
        return wait(lambda: (adapter(id_).get("job") or {}).get("state") == "ok", f"{id_} 检查完成", 60)

    newer = "9.10.0"
    reg.latest[ADAPTER_PACKAGES["claude"]] = newer
    check("claude")
    claude = adapter("claude")
    c(claude["latest"] == newer and claude["updateAvailable"], f"npm 上有新版本 {newer} 时标出可以升级")
    if ctx.app:
        st = ctx.app.until(lambda s: "claude" in s.get("adapterUpdates", []), "应用读到可以升级的适配器", "reload")
        c(st["adapterUpdates"] == ["claude"], f"应用侧栏提醒 1 个 ACP 适配器有新版本（{st['adapterUpdates']}）")
        ctx.app.do("open chats", "shot adapter-update-reminder")
        c(drawn(ctx.app.picture("adapter-update-reminder")), "应用：侧栏的适配器更新提醒画出来了（adapter-update-reminder.png）")

    reg.latest[ADAPTER_PACKAGES["claude"]] = "9.8.0"
    check("claude")
    c(not adapter("claude")["updateAvailable"], "装的版本比 npm 上的还新时不算有新版本")
    reg.latest.pop(ADAPTER_PACKAGES["claude"])
    check("claude")
    if ctx.app:
        ctx.app.until(lambda s: s.get("adapterUpdates") == [], "提醒随之消失", "reload")
        c(True, "应用：没有新版本后侧栏不再提醒")
