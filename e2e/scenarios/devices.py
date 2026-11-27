"""Pairing a browser or a phone, and phone access over the local network
(docs/operations/20261031-security.md#paired_devices)."""

import errno
import socket
import urllib.parse

from lib import ApiError, drawn, http, wait
from scenarios import scenario


def browser(ctx, method, path, body=None, cookie=None, addr=None, **kw):
    """A request as a page Janus served makes it: with its `Origin` and `Sec-Fetch-Site`, and the
    device cookie once it has one."""
    addr = addr or ctx.addr
    headers = {"Origin": f"http://{addr}", "Sec-Fetch-Site": "same-origin"}
    if cookie:
        headers["Cookie"] = f"janus_device={cookie}"
    return http(addr, method, path, body, headers=headers, **kw)


def refused(fn):
    try:
        fn()
    except ApiError as e:
        return e.code
    return 200


def wildcard_taken(port):
    """Something listens on every interface at `port`."""
    s = socket.socket()
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    try:
        s.bind(("0.0.0.0", port))
        return False
    except OSError as e:
        return e.errno == errno.EADDRINUSE
    finally:
        s.close()


@scenario("devices")
def devices(ctx, c):
    """手机访问：本机应用取配对码，浏览器带配对码配对后拿到设备 cookie，之后带 cookie 读收件箱、和本机应用读到的一样；没有 cookie、伪造的 cookie 都被拒；配对码只能用一次；应用的「设置 › 手机访问」列出配对的设备；打开手机访问后服务在局域网地址上也监听，那里同样要 cookie，自称本机的请求也不放行；取消配对后 cookie 立即失效；关掉后局域网上不再监听"""
    port = int(ctx.addr.rsplit(":", 1)[1])
    pairing = ctx.api("POST", "/api/devices/pairing")
    code = pairing["code"]
    c(len(code) == 10 and pairing["urls"][-1] == f"http://127.0.0.1:{port}/pair/{code}" and pairing["expiresAt"] > 0,
      f"本机应用取到配对码和链接（{pairing['urls'][-1]}）")
    c(refused(lambda: browser(ctx, "POST", "/api/devices/pairing")) == 401, "浏览器不能自己取配对码")
    c(refused(lambda: browser(ctx, "GET", "/api/inbox")) == 401, "没有配对的浏览器读不到收件箱（401）")
    c(refused(lambda: browser(ctx, "GET", "/api/inbox", cookie="0" * 64)) == 401, "伪造的 cookie 被拒")

    status, body, headers = browser(ctx, "POST", "/api/devices/claim", {"code": code, "name": "e2e 手机"}, raw=True)
    cookie = {k.lower(): v for k, v in headers.items()}.get("set-cookie", "")
    token = cookie.split(";")[0].split("=", 1)[1] if cookie.startswith("janus_device=") else ""
    c(status == 200 and token and "HttpOnly" in cookie and "SameSite=Strict" in cookie,
      f"浏览器用配对码配对，拿到 HttpOnly、SameSite=Strict 的设备 cookie（{cookie.split(';', 1)[-1].strip()}）")
    c(refused(lambda: browser(ctx, "POST", "/api/devices/claim", {"code": code, "name": "又一次"})) == 401, "同一个配对码不能再用")
    me = browser(ctx, "GET", "/api/devices/me", cookie=token)
    c(me["name"] == "e2e 手机", "带 cookie 的浏览器知道自己是哪台设备")
    mine = browser(ctx, "GET", "/api/inbox", cookie=token)
    c(mine.keys() == ctx.api("GET", "/api/inbox").keys() and len(mine["open"]) == len(ctx.api("GET", "/api/inbox")["open"]),
      "带 cookie 的浏览器读到的收件箱和本机应用读到的一样")
    c(refused(lambda: browser(ctx, "GET", "/api/connections/x/env", cookie=token)) == 403, "配对的浏览器也读不到 Agent 的环境变量")
    c(refused(lambda: http(ctx.addr, "POST", "/api/inbox/marks", {"key": "x/y"},
                           headers={"Origin": "https://evil.example", "Cookie": f"janus_device={token}"})) == 403,
      "别的网站借浏览器的 cookie 发来的修改被拒")

    if ctx.app:
        st = ctx.app.until(lambda s: any(d["name"] == "e2e 手机" for d in s.get("devices") or []),
                           "应用的手机访问页列出配对的设备", "open settings:devices", "shot devices-paired")
        c(st["lanAccess"]["enabled"] is False and st["lanAccess"]["listening"] is False, "手机访问默认关")
        c(drawn(ctx.app.picture("devices-paired")), "应用：手机访问页画出来了（devices-paired.png）")

    on = ctx.api("PUT", "/api/lan-access", {"enabled": True})
    c(on["enabled"] and on["listening"] and on["urls"][-1] == f"http://127.0.0.1:{port}", f"打开手机访问，服务在局域网上监听（{on['urls']}）")
    c(refused(lambda: browser(ctx, "PUT", "/api/lan-access", {"enabled": False}, cookie=token)) == 403, "配对的设备不能拨开关")
    c(wildcard_taken(port), "同一端口在所有接口上有人监听")
    lan = [urllib.parse.urlparse(u).netloc for u in on["urls"][:-1]]
    if lan:
        try:
            code_lan = refused(lambda: http(lan[0], "GET", "/api/inbox", headers={"Host": f"127.0.0.1:{port}"}, timeout=5))
            c(code_lan == 401, f"局域网地址上自称本机的请求也要 cookie（{lan[0]}，{code_lan}）")
            got = browser(ctx, "GET", "/api/inbox", cookie=token, addr=lan[0], timeout=5)
            c("open" in got, "手机带 cookie 从局域网地址读到收件箱")
        except OSError as e:
            # A server binary built just now waits on the macOS firewall's question before it takes
            # connections from the network; the listener itself was checked above.
            c(True, f"局域网连接被 macOS 防火墙挡着（{e}），只核对了监听")
    if ctx.app:
        st = ctx.app.until(lambda s: (s.get("lanAccess") or {}).get("listening") is True,
                           "应用的手机访问页跟着打开，显示配对二维码", "open settings:devices", "shot devices-lan")
        c(drawn(ctx.app.picture("devices-lan")), "应用：打开手机访问后的二维码画出来了（devices-lan.png）")

    ctx.api("DELETE", f"/api/devices/{me['id']}")
    c(refused(lambda: browser(ctx, "GET", "/api/inbox", cookie=token)) == 401, "取消配对后 cookie 立即失效")
    off = ctx.api("PUT", "/api/lan-access", {"enabled": False})
    c(not off["enabled"] and not off["listening"], "关掉手机访问")
    wait(lambda: not wildcard_taken(port), "局域网上不再监听", 10)
    c(True, "局域网上不再监听")
    if ctx.app:
        ctx.app.until(lambda s: not any(d["name"] == "e2e 手机" for d in s.get("devices") or []) and not (s.get("lanAccess") or {}).get("enabled"),
                      "应用的手机访问页跟着变：设备没了，开关关了", "open settings:devices")
        c(True, "应用跟着全局事件流的 devices 通知更新")
