"""本机环境: agents' HOME mirrors the person's home folder but a hidden list
(docs/operations/20261031-security.md#host_env)."""

import os

from lib import drawn, wait
from scenarios import scenario


@scenario("host-env")
def host_env(ctx, c):
    """本机环境：Agent 的用户目录默认是你用户目录的镜像（登录、钥匙串、工具配置都能用），只挡列出的路径：默认挡住常见 Agent 的配置和敏感数据；只有本机应用能读写；在应用里挡住一个路径，正在用的会话立刻看不到，移除后又能看到，本机的文件不动；同一条命令在本机和 Agent 环境里各跑一次，结果一致；恢复默认"""
    host = ctx.host_home
    files = {".config/gh/hosts.yml": "github.com:\n  user: e2e\n", ".lark-cli/config.json": "{}",
             "Library/Application Support/lark-cli/user.enc": "token", "Library/Keychains/login.keychain-db": "",
             ".codex/config.toml": "", ".agents/skills/x/SKILL.md": "", ".aws/credentials": "secret", ".zshrc": ""}
    for rel, text in files.items():
        os.makedirs(os.path.dirname(os.path.join(host, rel)), exist_ok=True)
        with open(os.path.join(host, rel), "w") as f:
            f.write(text)

    for method, path, body in (("GET", "/api/host-env", None), ("PUT", "/api/host-env", {"paths": []}),
                               ("POST", "/api/host-env/try", {"command": "true"})):
        code, _ = ctx.api_error(method, path, body, headers={"Origin": "https://evil.example"})
        c(code in (401, 403), f"网页发来的 {method} {path} 被拒绝，只给本机的应用（{code}）")
    v = ctx.api("GET", "/api/host-env")
    hidden = {h["path"]: h for h in v["hidden"]}
    c(v["defaults"] and hidden[".codex"]["why"] == "agent" and hidden[".codex"]["here"] and hidden[".agents"]["why"] == "agent"
      and hidden[".aws"]["why"] == "sensitive" and ".ssh" not in hidden,
      "默认挡住常见 Agent 的配置和敏感数据，~/.ssh 不挡")

    # A session opened now: its home folder is the person's, but what is hidden.
    t = ctx.team("hostenv", members=("m1",))
    chat = ctx.api("POST", "/api/chats", {"role": t.member("m1")})["id"]
    wait(lambda: [e for e in ctx.api("GET", f"/api/runs/{chat}")["events"] if e["type"] == "agent.status" and e["data"].get("turn") == "idle"],
         "会话打开", 60)
    home = os.path.join(ctx.home, "data", "agents", t.member_conn["id"], "home")

    def same(rel):
        p = os.path.join(home, rel)
        return os.path.islink(p) and os.path.realpath(p) == os.path.realpath(os.path.join(host, rel))

    c(same(".config/gh") and same(".lark-cli") and same("Library/Application Support/lark-cli") and same("Library/Keychains")
      and same(".zshrc") and not os.path.lexists(os.path.join(home, ".codex")) and not os.path.lexists(os.path.join(home, ".agents"))
      and not os.path.lexists(os.path.join(home, ".aws")),
      "会话的用户目录里，登录、钥匙串和配置都链接到本机的同一份，Agent 的配置和 ~/.aws 不在")

    if ctx.app:
        st = ctx.app.until(lambda s: ".config/gh" in ((s.get("hostEnv") or {}).get("hidden") or []),
                           "应用里挡住 ~/.config/gh", "open settings:host-env", "host-hide .config/gh on")
        c(st["hostEnv"]["defaults"] is False, "改过之后不再是默认列表")
    else:
        ctx.api("PUT", "/api/host-env", {"paths": [h for h in hidden] + [".config/gh"]})
    c(not os.path.lexists(os.path.join(home, ".config/gh")) and os.path.exists(os.path.join(host, ".config/gh/hosts.yml")),
      "挡住后，正在用的会话立刻看不到，本机的文件不动")

    tried = ctx.api("POST", "/api/host-env/try", {"command": "cat ~/.zshrc && cat ~/.aws/credentials"})
    c(tried["host"]["code"] == 0 and "secret" in tried["host"]["output"] and tried["agent"]["code"] != 0,
      f"同一条命令本机能读 ~/.aws，Agent 环境里读不到（{tried['host']['code']} / {tried['agent']['code']}）")
    if ctx.app:
        st = ctx.app.until(lambda s: (s.get("hostTry") or {}).get("agent", {}).get("code") == 0, "应用里试一条命令，两边都成功",
                           "host-try cat ~/.lark-cli/config.json")
        ctx.app.do("shot host-env")
        c(st["hostTry"]["host"]["code"] == 0 and drawn(ctx.app.picture("host-env")), "应用：本机环境页和试命令的结果画出来了（host-env.png）")
        ctx.app.until(lambda s: ".config/gh" not in ((s.get("hostEnv") or {}).get("hidden") or []), "应用里移除 ~/.config/gh", "host-hide .config/gh off")
    else:
        ctx.api("PUT", "/api/host-env", {"paths": [h for h in hidden]})
    c(same(".config/gh"), "移除后，会话又能看到 ~/.config/gh")
    code, why = ctx.api_error("PUT", "/api/host-env", {"paths": [".config", ".config/gh"]})
    c(code == 400 and "里面" in why, f"一个路径在另一个里面时被拒绝（{code}）")
    v = ctx.api("PUT", "/api/host-env", {"paths": None})
    c(v["defaults"] and {h["path"] for h in v["hidden"]} == set(hidden), "恢复默认")
