"""The app on a Janus home with no server running: it starts one, and stops it when it quits."""

import os

from lib import App, ApiError, free_addr, http, wait
from scenarios import scenario

ADDR = free_addr()


@scenario("app-server", serial=True)
def app_server(ctx, c):
    """应用自己启动服务：没有服务在运行时，应用启动服务程序（打包版用内置的），退出时把它停掉"""
    if not ctx.app:
        c(True, "（--no-app：跳过）")
        return
    home = ctx.path("app-server", "home")
    # From source the app looks for JANUS_SERVER_BIN; Janus.app runs the server inside the bundle.
    env = {} if ctx.packaged else {"JANUS_SERVER_BIN": ctx.server_bin, "JANUS_NODE": ctx.node}
    app = App(ctx.app_bin, home, ADDR, os.path.join(ctx.report, "app-server"), os.path.join(ctx.report, "app-server.log"), env)
    try:
        st = app.until(lambda s: s.get("connections") is not None, "应用连上自己启动的服务", timeout=90)
        meta = http(ADDR, "GET", "/api/meta")
        c(os.path.realpath(meta["dataDir"]) == os.path.realpath(home), f"应用在 {ADDR} 上为这个 Janus home 启动了服务")
        c({"coordinator", "implementer"} <= {r["id"] for r in st["roles"]}, "应用读到新 home 的默认数字员工")
        c(os.path.exists(os.path.join(home, "janus.db")), "数据写进这个 Janus home")
        app.do("shot first-launch")
    finally:
        app.quit()

    def stopped():
        try:
            http(ADDR, "GET", "/api/meta", timeout=1)
            return False
        except (OSError, ApiError):
            return True
    wait(stopped, "应用退出后服务停止", 20)
    c(True, "应用退出后，它启动的服务也停止了")
