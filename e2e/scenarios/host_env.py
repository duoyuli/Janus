"""本机环境: what of the person's own home folder agents may use (docs/operations/20261031-security.md#host_env)."""

import os

from lib import drawn, wait
from scenarios import scenario

KEYCHAIN = "Library/Keychains"


@scenario("host-env")
def host_env(ctx, c):
    """本机环境：只列本机有的工具和家目录里的其他内容，不列 Agent 自己的配置；只有本机应用能读写，Agent 自己的配置被拒；在应用里打开 GitHub CLI 时连同钥匙串一起共享，新开的会话的家目录里链接到本机的同一份，关掉后撤回且不动本机的文件"""
    host = ctx.host_home
    files = {".config/gh/hosts.yml": "github.com:\n  user: e2e\n", ".lark-cli/config.json": "{}", f"{KEYCHAIN}/login.keychain-db": "",
             ".codex/config.toml": "", ".zshrc": "", ".zsh_history": ""}
    for rel, text in files.items():
        os.makedirs(os.path.dirname(os.path.join(host, rel)), exist_ok=True)
        with open(os.path.join(host, rel), "w") as f:
            f.write(text)

    for method, body in (("GET", None), ("PUT", {"paths": [".config/gh"]})):
        code, _ = ctx.api_error(method, "/api/host-env", body, headers={"Origin": "https://evil.example"})
        c(code == 403, f"网页发来的 {method} 被拒绝，本机环境只给本机的应用（{code}）")
    v = ctx.api("GET", "/api/host-env")
    ids = [t["id"] for t in v["tools"]]
    c({"keychain", "gh", "lark"} <= set(ids) and "codex" not in ids and ".zshrc" in v["others"]
      and not {".codex", ".zsh_history"} & set(v["others"]), f"只列本机有的工具和家目录里的其他内容，不列 Agent 自己的配置和历史（{ids}，{v['others']}）")
    code, why = ctx.api_error("PUT", "/api/host-env", {"paths": [".codex"]})
    c(code == 400 and "Agent 自己的配置" in why, f"Agent 自己的配置不能共享（{code}）")

    t = ctx.team("hostenv", members=("m1",))
    want = {".config/gh", KEYCHAIN}
    if ctx.app:
        st = ctx.app.until(lambda s: set((s.get("hostEnv") or {}).get("shared") or []) == want,
                           "应用里打开 GitHub CLI，连同钥匙串一起共享", "open settings:hostEnv", "host-tool gh on")
        ctx.app.do("shot host-env")
        c(drawn(ctx.app.picture("host-env")) and "gh" in st["hostEnv"]["tools"], "应用：本机环境页画出来了（host-env.png）")
    else:
        ctx.api("PUT", "/api/host-env", {"paths": sorted(want)})
    c(set(ctx.api("GET", "/api/host-env")["shared"]) == want, "服务端记下的和应用里打开的一致")

    # A session opened now finds the person's own sign-in in its home folder.
    chat = ctx.api("POST", "/api/chats", {"role": t.member("m1")})["id"]
    wait(lambda: [e for e in ctx.api("GET", f"/api/runs/{chat}")["events"] if e["type"] == "agent.status" and e["data"].get("turn") == "idle"],
         "会话打开", 60)
    sandbox = os.path.join(ctx.home, "data", "agents", t.member_conn["id"], "home")
    gh = os.path.join(sandbox, ".config", "gh")
    c(os.path.islink(gh) and os.path.realpath(gh) == os.path.realpath(os.path.join(host, ".config", "gh"))
      and open(os.path.join(gh, "hosts.yml")).read() == files[".config/gh/hosts.yml"] and os.path.islink(os.path.join(sandbox, KEYCHAIN)),
      "新开的会话的家目录里，~/.config/gh 和钥匙串链接到本机的同一份")

    if ctx.app:
        ctx.app.until(lambda s: (s.get("hostEnv") or {}).get("shared") == [KEYCHAIN], "应用里关掉 GitHub CLI", "host-tool gh off")
    else:
        ctx.api("PUT", "/api/host-env", {"paths": [KEYCHAIN]})
    c(not os.path.lexists(gh) and os.path.exists(os.path.join(host, ".config/gh/hosts.yml")),
      "关掉后会话的家目录里撤回链接，本机的文件不动")
    ctx.api("PUT", "/api/host-env", {"paths": []})
