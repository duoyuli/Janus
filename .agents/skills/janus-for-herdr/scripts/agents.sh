#!/bin/bash
# agents.sh：一键检查本机装了哪些可以当协作者的 Agent。只读，不启动任何 Agent。
# 用法：bash agents.sh
# 名单是 Herdr「Start an agent」菜单支持的 Agent 和它们的启动命令（2026-11-26）。
# Herdr 支持了新的 Agent 时，在下面的名单里加一行：显示名|命令|启动时要带的参数。
AGENTS='
Claude Code|claude|
Codex|codex|--no-daemon
Pi|pi|
OpenCode|opencode|
Cursor|agent|
Grok|grok|
Antigravity|agy|
Copilot|copilot|
Amp|amp|
Gemini|gemini|
Crush|crush|
DeepSeek Harness|dsh-tui|
goose|goose|
Hermes|hermes|
Kimi|kimi|
Kiro|kiro-cli|
oh-my-pi|omp|
PiG|pig|
Qoder|qodercli|
'

# 版本号最多等 3 秒，免得某个命令卡住。
version() { perl -e 'alarm 3; exec @ARGV' "$1" --version 2>/dev/null | head -1 | cut -c1-40; }

echo "Herdr:"
for tool in hc herdr-app; do
  if path=$(command -v "${tool}"); then
    v=""; [ "${tool}" = hc ] && v=$(version hc)
    printf '  %-18s %s  %s\n' "${tool}" "${path}" "${v}"
  else
    printf '  %-18s 没有安装\n' "${tool}"
  fi
done
if [ -n "${HC_TERMINAL_ID}" ]; then
  echo "  本会话的终端地址：${HC_TERMINAL_ID}"
else
  echo "  本会话不在 hc 的终端里（HC_TERMINAL_ID 为空），协作者没法把消息送进来"
fi

# 版本一栏用来确认命令确实是那个 Agent：agent 这样的通用名字可能被别的工具占用。
installed=""; missing=""
while IFS='|' read -r name cmd args; do
  [ -n "${name}" ] || continue
  if path=$(command -v "${cmd}"); then
    installed="${installed}$(printf '  %-18s %-22s %-28s %s' "${name}" "${cmd}${args:+ ${args}}" "$(version "${cmd}")" "${path}")
"
  else
    missing="${missing}  ${name} (${cmd})
"
  fi
done <<LIST
${AGENTS}
LIST

echo
echo "已安装（显示名、启动命令、版本、路径）："
printf '%s' "${installed:-  （无）
}"
echo
echo "没有安装："
printf '%s' "${missing:-  （无）
}"
