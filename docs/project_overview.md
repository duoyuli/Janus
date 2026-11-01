# 项目概览

本文介绍 Janus 的目标、核心术语、仓库结构和常用命令。组件之间如何协作见[架构概览](architecture/overview.md)。

## 目标

Janus 是本地优先的多 Agent 协作工作空间。它通过 ACP（Agent Client Protocol）连接 Codex、Claude Code、Grok、Pi 和其他兼容 ACP 的 Agent，让一个主控 Agent 带着一组协作者完成用户给出的目标，并把每一次会话、编排、审批、产物和交付都记录成可回放的事件。

Janus 是单机、单用户的工具：编排器只在本机运行，任务直接修改用户选择的目录。

<a id="glossary"></a>
## 核心术语

| 术语 | 含义 | 代码中的名字 |
| --- | --- | --- |
| Agent / Provider 连接 | 一个可启动的编码 Agent，带独立配置目录和凭据 | `connection`（`configs.kind`）、`RuntimeSpec` |
| 基础运行时 | Agent 的底层类型：`codex`、`claude`、`grok`、`pi`、`custom` | 连接的 `base` 字段 |
| 角色 | 绑定一个 Agent，指定名字、职责、模型、推理强度、会话控制项、Skill、MCP | `role` |
| 会话控制项 | Agent 在 ACP 会话中声明的可选项，例如 Codex 的沙箱预设、Claude Code 的权限模式 | `controls`、角色的 `session` |
| 权限模式 | 会话控制项中决定 Agent 能做什么的那些，角色编辑器的「权限」只显示它们 | `permission: true` 的控制项 |
| 工作区 | 一个本机目录加一组角色成员，其中一名是主控 | `workspace` |
| Run | 在工作区中提交的一个目标，从创建到验收的全过程 | `Run`、`runs` 表 |
| 成员 | Run 创建时角色的快照；主控的 id 固定为 `lead` | `Member` |
| 主控 | 用 `/goal` 推进目标的 Agent，只能是 Claude Code 或 Codex | `lead_capable` |
| 协作者 | 主控以外的成员 | |
| 编排 | 主控提交的一组任务 | `orchestrate` 工具、`orchestration.*` 事件 |
| 共享看板 | 主控和协作者交换任务、进展、阻塞、提问的通道，底层是事件日志 | `board_*` 函数 |
| 任务 / 尝试 | 任务是看板上的一项工作；每次执行是一次尝试，对应一个新的 ACP 会话 | `TaskDef`、`attempts` 表 |
| 产物 | 尝试结束时保存的回复和 diff | `artifacts` 表 |

## 仓库结构

```text
Janus/
├── AGENTS.md              # 智能体指令（CLAUDE.md 是指向它的符号链接）
├── README.md              # 产品入口
├── justfile               # 开发、构建、安装和服务命令
├── docs/                  # 本文档库
├── backend/               # Rust 编排器（Cargo 包名 janus）
│   ├── Cargo.toml
│   ├── src/
│   │   ├── main.rs        # 启动、HTTP 路由
│   │   ├── config.rs      # Janus home、配置项、Janus 日志
│   │   ├── adapters.rs    # ACP 适配器的按需安装、升级与回滚
│   │   ├── acp.rs         # ACP 客户端
│   │   ├── agents.rs      # Agent、角色、工作区、Skill、会话控制项
│   │   ├── orchestrator.rs# Run、目标模式、看板、调度、验证、交付、恢复、集成测试
│   │   └── db.rs          # SQLite
│   └── testdata/fake_acp.py  # 测试用脚本化 ACP Agent
├── frontend/              # macOS 应用（SwiftUI，Swift Package）
│   ├── Sources/Janus/     # Core/（JSON、API、服务进程、事件推导）和 UI/
│   ├── Tests/JanusTests/  # Swift Testing 测试
│   └── scripts/           # package.sh（打包 Janus.app 和 DMG）、make-icon.swift
├── .github/workflows/     # CI（测试）和 Release（打包 DMG 并发布）
├── examples/calc/         # 最小示例工作区（自带 .git）
└── .agents/skills/        # 项目 skill（.claude/skills 是指向它的符号链接）
```

仓库只放代码。数据库、ACP 适配器、产物、配置和日志都在 Janus home（默认 `~/.janus`），见[部署方案](operations/deployment.md#layout)。`.gitignore` 只忽略构建输出：`backend/target/`、`frontend/.build/`、`dist/` 和 `.DS_Store`。

## 常用命令

在仓库根目录执行，`just` 列出全部命令：

| 目的 | 命令 |
| --- | --- |
| 从源码启动（开发） | `just dev` |
| 运行全部测试 | `just test`；macOS 应用 `just test-mac` |
| 从源码运行 macOS 应用 | `just mac` |
| 打包 Janus.app 和 DMG | `just dmg` |
| 安装并以服务方式启动 | `just install`，然后 `just start` |
| 查看服务状态和日志 | `just status`、`just logs` |
| 验证示例工作区 | `python3 -m unittest discover -s examples/calc` |

部署见[部署方案](operations/deployment.md)，排障见[运行手册](operations/runbook.md)，测试结构见[测试](operations/testing.md)。
