# Janus 的构建、部署和服务管理。`just` 列出全部命令，说明见 docs/operations/deployment.md。
# Janus 保存的一切都在 JANUS_HOME（默认 ~/.janus），仓库只放代码。

set shell := ["bash", "-euo", "pipefail", "-c"]

janus_home := env("JANUS_HOME", home_directory() / ".janus")
bin := janus_home / "bin" / "janus"
label := "local.janus"
plist := home_directory() / "Library" / "LaunchAgents" / label + ".plist"

# 列出全部命令
default:
    @just --list --unsorted

# 开发：从源码启动，页面直接读 frontend/app.html，改了刷新即可
dev:
    cd backend && cargo run

# 运行全部测试
test:
    cd backend && cargo test

# @project-doc docs/operations/deployment.md#macos_app
# 从源码运行 macOS 原生应用（连接已在运行的 Janus，否则启动 JANUS_HOME/bin/janus）
mac:
    cd macos && swift run Janus

# macOS 应用的测试（Swift Testing，需要安装 Xcode）
test-mac:
    cd macos && swift test

# 构建 dist/Janus.app（原生界面 + 内置服务，本机架构，不打 DMG）
app:
    macos/scripts/package.sh --no-dmg

# 构建 dist/Janus.app 和 dist/Janus-<版本>.dmg；发布用的通用版本由 CI 构建
dmg:
    macos/scripts/package.sh

# @project-doc docs/operations/deployment.md#install
# 构建 release 二进制并安装到 JANUS_HOME/bin/janus（升级 Janus 也用它，之后 just restart）
install:
    #!/usr/bin/env bash
    set -euo pipefail
    command -v cargo >/dev/null || { echo "找不到 cargo：先安装 Rust（https://rustup.rs）"; exit 1; }
    (cd backend && cargo build --release --locked)
    mkdir -p "{{janus_home}}/bin"
    install -m 755 backend/target/release/janus "{{bin}}.new"
    mv -f "{{bin}}.new" "{{bin}}"
    echo "已安装 $("{{bin}}" --version) → {{bin}}"
    command -v node >/dev/null || echo "提醒：PATH 里没有 node，ACP 适配器需要 Node 22.19 以上"
    if launchctl print "gui/$(id -u)/{{label}}" >/dev/null 2>&1; then echo "服务还在跑旧版本：执行 just restart"; else echo "下一步：just start 以服务方式启动，或 just run 在前台运行"; fi

# 以 launchd 服务启动：登录时自动启动，异常退出后自动重启（macOS）
start: _macos
    #!/usr/bin/env bash
    set -euo pipefail
    [ -x "{{bin}}" ] || { echo "还没有安装：先执行 just install"; exit 1; }
    just _write-plist "{{plist}}"
    launchctl bootout "gui/$(id -u)/{{label}}" 2>/dev/null || true
    launchctl bootstrap "gui/$(id -u)" "{{plist}}"
    echo "已启动：http://$(just _addr)"

# 停止服务并取消登录自启
stop: _macos
    #!/usr/bin/env bash
    if launchctl print "gui/$(id -u)/{{label}}" >/dev/null 2>&1; then launchctl bootout "gui/$(id -u)/{{label}}"; echo "已停止"; else echo "服务没有在运行"; fi

# 重启服务（just install 升级二进制之后执行）
restart: _macos
    launchctl kickstart -k "gui/$(id -u)/{{label}}" && echo "已重启"

# 服务状态，以及 HTTP 服务是否响应
status:
    #!/usr/bin/env bash
    if launchctl print "gui/$(id -u)/{{label}}" >/dev/null 2>&1; then
      launchctl print "gui/$(id -u)/{{label}}" | sed -n 's/^[[:space:]]*\(state\|pid\|last exit code\) = /  \1: /p'
    else
      echo "服务没有注册"
    fi
    addr="$(just _addr)"
    if curl -fsS -o /dev/null "http://$addr/api/meta"; then echo "http://$addr 正常响应"; else echo "http://$addr 没有响应"; fi

# 持续查看 Janus 日志
logs:
    mkdir -p "{{janus_home}}/logs" && touch "{{janus_home}}/logs/janus.log"
    tail -n 50 -f "{{janus_home}}/logs/janus.log"

# 在前台运行已安装的二进制
run:
    #!/usr/bin/env bash
    [ -x "{{bin}}" ] || { echo "还没有安装：先执行 just install"; exit 1; }
    exec "{{bin}}"

# 停止服务并删除二进制；数据仍保留在 JANUS_HOME
uninstall:
    #!/usr/bin/env bash
    if [ "$(uname)" = "Darwin" ]; then launchctl bootout "gui/$(id -u)/{{label}}" 2>/dev/null || true; rm -f "{{plist}}"; fi
    rm -f "{{bin}}"
    echo "已删除服务和 {{bin}}。数据仍在 {{janus_home}}，要彻底删除请自己删掉这个目录。"

# 打印 launchd 服务配置（不安装）
plist:
    @just _write-plist /dev/stdout

# @project-doc docs/operations/deployment.md#run_as_service
# launchd 启动的进程 PATH 很短，找不到 node、codex、claude、grok、git、npm，
# 所以服务沿用执行 just start 时终端的 PATH。
_write-plist target:
    #!/usr/bin/env bash
    set -euo pipefail
    xml() { sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g' <<<"$1"; }
    [ "{{target}}" = /dev/stdout ] || mkdir -p "$(dirname "{{target}}")"
    mkdir -p "{{janus_home}}/logs"
    cat >"{{target}}" <<EOF
    <?xml version="1.0" encoding="UTF-8"?>
    <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
    <plist version="1.0">
    <dict>
      <key>Label</key><string>{{label}}</string>
      <key>ProgramArguments</key><array><string>$(xml "{{bin}}")</string></array>
      <key>EnvironmentVariables</key>
      <dict>
        <key>PATH</key><string>$(xml "$PATH")</string>
        <key>JANUS_HOME</key><string>$(xml "{{janus_home}}")</string>
      </dict>
      <key>RunAtLoad</key><true/>
      <key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>
      <key>ThrottleInterval</key><integer>10</integer>
      <key>StandardOutPath</key><string>$(xml "{{janus_home}}/logs/janus.out.log")</string>
      <key>StandardErrorPath</key><string>$(xml "{{janus_home}}/logs/janus.out.log")</string>
    </dict>
    </plist>
    EOF

# 监听地址：JANUS_ADDR，否则 config.toml 的 addr，否则默认值
_addr:
    #!/usr/bin/env bash
    file="$(sed -n 's/^addr *= *"\(.*\)"/\1/p' "{{janus_home}}/config.toml" 2>/dev/null | head -1)"
    echo "${JANUS_ADDR:-${file:-127.0.0.1:7878}}"

_macos:
    @[ "$(uname)" = "Darwin" ] || { echo "服务命令使用 launchd（macOS）；其他系统用 just run 配合自己的进程管理器，见 docs/operations/deployment.md"; exit 1; }
