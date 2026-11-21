# ADR：Janus home 按「丢了能不能再有」分类，路径只从 `Layout` 取

- 状态：已采纳（accepted），记录于 2026-11-21。取代 [ADR：用户数据、依赖和日志都放在 Janus home](20261031-user_home_layout.md) 中目录怎样划分的部分，「仓库只放代码、一切都在 Janus home」不变。
- 相关文档：[部署方案：目录规范](../operations/20261031-deployment.md#layout)、[新增文件放在哪里](../operations/20261031-deployment.md#new_files)、[数据模型与存储](../architecture/20261031-data_model.md#locations)、[安全边界](../operations/20261031-security.md#secrets)。

## 背景

功能一个个加进来时，各自在 Janus home 的顶层建目录：到数据版本 13 时顶层有 13 项，看名字分不出哪些要备份、哪些能删。路径在后端 12 个文件里各自拼字符串，应用里又各自解析了三次 `JANUS_HOME`，目录规范只写在文档里，没有测试守着。2026-11-20 的整理找到了这样积累下来的问题：

- 一个 Run 的东西分在 `artifacts/`、`logs/`、`runs/`、`chats/`、`worktrees/` 五个顶层目录，删除 Run 靠手写的子目录清单，对话还要多删一个。
- 只增不减：`updates/` 里留着 25 个旧版本，Agent 的 `tmp/` 从不清理（本机 120 MB）。
- 删除 Run 和对话时，Agent 自己存下的会话记录留在它的配置目录里，用户删掉的对话全文还在。
- 同一项 MCP 资源的东西分在四处，日志按名字命名，工具列表从文件名的前缀认 id。
- Agent 目录里名为 `home/` 的是 Agent 的配置目录，名为 `root/` 的才是 HOME。
- 发版用的签名私钥也放在 Janus home。

## 决策

1. **顶层按丢失的代价分类**：`data/`（无法再有，备份只需它）、`work/`（Agent 读写的目录）、`cache/`（停止 Janus 后可以整个删掉，用到时重新生成）、`tmp/`（每次启动清空）、`logs/`、`backups/`，加上 `bin/`、`config.toml`、`janus.pid`。顶层只能出现这些名字（`layout::TOP`）。
2. **对象的东西放在对象的 id 下**：Agent、Run、对话、资源各有自己的目录；跨分类的部分（凭据、临时目录、日志）也按 id 命名。每类对象有一个列出它全部路径的方法（`agent_paths`、`item_paths`、`RunDirs`），删除对象就是删除这些路径。
3. **任务和对话在目录上分开**：`data/runs/`、`work/runs/`、`logs/runs/` 和 `data/chats/`、`work/chats/`、`logs/chats/`，由 `runs.kind` 决定；数据模型、接口和其余代码不变。
4. **凭据集中在 `data/secrets/`**：Agent 的和资源库 MCP 服务器的都在这里，目录 `0700`、文件 `0600`。Agent 原生的登录状态（如 `auth.json`）只能留在它自己的配置目录。
5. **路径只从一处取**：后端是 `backend/src/layout.rs` 的 `Layout`，由 `App.cfg` 持有；应用是 `JanusHome`。其他代码不在 home 上拼名字，迁移里写死的旧路径除外。
6. **删除 Run 也删除 Agent 原生的会话记录**：按事件和尝试记录里的会话 id，删掉 Claude Code 的 `projects/…/<会话>.jsonl` 和 Codex 的 `sessions/…/rollout-…-<会话>.jsonl`。
7. **会增长的都有回收**：`tmp/` 启动时清空，`cache/updates/` 只留正在下载的版本和 `previous/`，`cache/mcp-tools/<资源>/` 只留当前 revision，`backups/` 留 5 份。
8. **移动是一种数据升级**：数据版本 14 把旧布局搬进新布局（同一文件系统内的 `rename`），数据库、事件，以及 Agent 和 MCP 服务器自己写下的文件（配置、会话记录、在自己 HOME 里装的工具链，文本文件和符号链接）里记下的旧路径按前缀改成新路径，Claude Code 的项目目录随工作目录改名，worktree 用 `git worktree repair` 让原仓库知道新位置。以后改布局也这样做：加一条迁移，用 `backend/src/layout/relocate.rs` 的 `Relocation` 列出移动和删除，由它完成改名和改写。
9. **签名私钥不在 Janus home**：`make-signing-cert.sh` 默认写到 `~/.config/janus/signing/`。

## 取舍

- **改写事件里的路径**：事件是只追加的历史，迁移原则上不改它。这里改的是文件现在在哪，不是发生了什么；不改的话，历史里的产物、附件和工具调用里的路径都指向不存在的位置。只按完整的路径前缀替换，`…/logs/r1` 不会匹配 `…/logs/r10`。
- **不存相对路径**：Agent 收到的提示、事件和会话记录都需要绝对路径才能直接打开。路径照旧按绝对路径记，布局变化时由迁移统一改写。
- **改写 Agent 自己的文件**：这些文件在 Janus 为 Agent 隔离出的目录里，属于 Janus 管理的数据；不改的话，继续旧会话时 Agent 看到的路径都不存在，它在 HOME 里装的工具（例如写死了绝对路径的 `.cargo/env`、指向绝对路径的符号链接）也会失效。超过 64 MB 的文件和不是文本的文件不改。用户自己的 `~/.claude`、`~/.codex` 不受影响。
- **退回旧版本**：旧版本在顶层找不到 `janus.db`，会建一个空库。退回前要把 `backups/` 中升级前的备份放回顶层的 `janus.db`，见[升级](../operations/20261031-deployment.md#upgrade)。

## 后果

- 新功能要写盘时，按[新增文件放在哪里](../operations/20261031-deployment.md#new_files)选好位置，在 `Layout`（应用用到时在 `JanusHome`）里加一个方法，更新目录规范；端到端测试检查 home 顶层只有声明过的名字、删除对象后不留下带它 id 的路径。
- 删除 `data/` 以外的任何目录都不会丢失无法恢复的东西（`work/` 中没提交的改动除外，删除 Run 时它们先存成补丁）。
