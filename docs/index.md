# Janus 工程文档

> 这是 Project Doc 根索引。加载 project-doc skill 后，在本仓库的第一个任务中完整阅读本文件，再按「阅读时机」进入相关分类。已读内容按 skill 的「阅读与上下文复用」规则复用；内容缺失或需要刷新时再重读。

## 项目

Janus 是本地优先的多 Agent 协作工作空间。它通过 ACP（Agent Client Protocol）连接 Codex 和 Claude Code，让一个负责人 Agent 带着一组协作者完成用户给出的目标，并把每一次会话、编排、审批、产物和交付都记录成可回放的事件。

Janus 是单机、单用户的工具：编排器只在本机运行。Git 仓库上的每个任务在自己的 worktree 中修改文件，验收时由 Janus 提交并合并回任务开始时的分支；非 Git 目录中任务直接修改用户选择的目录。

<a id="glossary"></a>
## 核心术语

| 术语 | 含义 | 代码中的名字 |
| --- | --- | --- |
| Agent / Provider 连接 | 一个可启动的编码 Agent，带独立配置目录和凭据 | `connection`（`configs.kind`）、`RuntimeSpec` |
| 基础运行时 | Agent 的底层类型：`codex` 或 `claude` | 连接的 `base` 字段、`Runtime` |
| 角色 | 绑定一个 Agent，指定名字、职责、模型、推理强度、会话控制项和指令；资源库的 Skill 和 MCP 给所有角色，不按角色选择；应用界面中叫「数字员工」 | `role` |
| 会话控制项 | Agent 在 ACP 会话中声明的可选项，例如 Codex 的沙箱预设、Claude Code 的权限模式 | `controls`、角色的 `session` |
| 权限模式 | 会话控制项中决定 Agent 能做什么的那些，角色编辑器的「权限」只显示它们 | `permission: true` 的控制项 |
| 工作区 | 一个本机目录加一组角色成员，其中一名是负责人 | `workspace` |
| Run | `runs` 表中的一条记录，分两类（`kind`）：在工作区中提交的一个目标，从创建到验收的全过程，应用界面中叫「任务」；和一名数字员工的[对话](domains/20261120-chats.md)，没有工作区和验收 | `Run`、`runs` 表 |
| 成员 | Run 创建时角色的快照；负责人的 id 固定为 `lead` | `Member` |
| 负责人 | 用 `/goal` 推进目标的 Agent（Claude Code 或 Codex） | 成员的 `lead` |
| 协作者 | 负责人以外的成员 | |
| 编排 | 负责人提交的一组任务 | `orchestrate` 工具、`orchestration.*` 事件 |
| 共享看板 | 负责人和协作者交换任务、进展、阻塞、提问的通道，底层是事件日志 | `board_*` 函数 |
| 任务 / 尝试 | 任务是看板上的一项工作，应用界面中叫「子任务」；每次执行是一次尝试，对应一个新的 ACP 会话 | `TaskDef`、`attempts` 表 |
| 产物 | 尝试结束时保存的回复和 diff | `artifacts` 表 |

## 仓库结构

- `backend/`：Rust 编排器（Cargo 包名 `janus`），各模块的职责见[模块与启动顺序](architecture/20261116-modules.md#modules)。`backend/roles/` 是默认数字员工，一人一个 Markdown 文件（元数据加指令），格式见其中的 `README.md`。`backend/testdata/` 放测试用的脚本化 ACP Agent、前后端共用的用量用例、数据结构快照和旧版本写出的样本数据。
- `frontend/`：macOS 应用（SwiftUI，Swift Package），源码结构见 [macOS 应用](architecture/20261031-frontend.md#source_layout)。`frontend/scripts/` 放打包脚本。
- `e2e/`：端到端验证脚本（Python 标准库）：`product.py` 在真实构建的服务和应用上跑每条用户旅程，场景在 `e2e/scenarios/`；`upgrade.py` 是数据升级测试；`lib.py` 是两者共用的部分。见[端到端验证](operations/20261031-testing.md#e2e)。
- `docs/`：本文档库。`README.md` 是产品入口，`CHANGELOG.md` 是更新记录，`LESSONS.md` 是清理 Agent 合并的代码时积累的经验教训和 `AGENTS.md` 每条合并规则的来由，它们都在仓库根目录，不属于本库。
- `justfile`：开发、构建、安装和服务命令，见[部署方案](operations/20261031-deployment.md)。
- `.github/workflows/`：测试和发布工作流，见 [CI 与发布](operations/20261101-ci_release.md)。
- `examples/calc/`：最小示例项目（普通文件，随仓库提交），用作工作区前先复制到仓库之外。

仓库只放代码。数据库、ACP 适配器、产物、配置和日志都在 Janus home（默认 `~/.janus`），见[部署方案](operations/20261031-deployment.md#layout)。`.gitignore` 只忽略构建输出：`backend/target/`、`frontend/.build/`、`dist/` 和 `.DS_Store`。

## 文档范围

本库记录需要跨多个代码位置才能理解的 Janus 工程知识，只写当前的工程事实。单个函数就能看懂的实现细节、工作进度和临时方案不在本库内。代码中的 `@project-doc docs/<路径>.md#<章节 ID>` 注释指向解释该代码的章节，修改那段代码前先读对应章节。

文档命名为 `YYYYMMDD-<主题>.md`，日期是文档的创建日期（报告和 ADR 是报告日或决策日），之后内容更新不改文件名；`index.md` 不加日期。

## 分类

- [架构](architecture/index.md)：设计原则、组件与一次 Run 的数据流（写在分类索引中），模块与启动顺序、存储布局、ACP 适配器管理、macOS 应用。阅读时机：修改模块边界、启动流程、SQLite 表、数据目录、适配器安装与升级，新增内置运行时，或改动 `frontend/` 的状态推导和页面结构。
- [领域](domains/index.md)：协作三方（负责人、协作者和编排器各能做什么）、Run 生命周期、目标模式与共享看板、Agent / 角色 / 工作区与权限模式、资源库。阅读时机：涉及 Run 或任务状态、调度、重试、验证、交付、恢复、负责人行为、协作者提问、权限模式、角色配置，或角色的 Skill 和 MCP 资源。
- [接口](interfaces/index.md)：HTTP API、事件、看板 MCP 工具、ACP 集成和配置项。阅读时机：改动路由、Run 操作、事件类型或字段、MCP 工具、ACP 请求与 `_meta`、环境变量或 `config.toml`。
- [运维](operations/index.md)：部署方案（目录规范、安装、服务、升级）、CI 与发布、运行手册、测试、安全边界。阅读时机：安装、启动、升级或卸载 Janus，修改 `justfile`、CI 工作流或打包脚本，发布版本，排查失败的 Run，运行或编写测试，评估隔离与凭据风险。
- [决策](decisions/index.md)：架构决策记录（ADR）。阅读时机：想改变负责人模式、工作目录模型、权限设计、看板通道、事件存储、Agent 隔离方式、数据存放位置或适配器安装方式之前。
- [研究](research/index.md)：外部多 Agent 协作与 Agent Team 的定期观察报告，以及给本项目的借鉴建议。阅读时机：评估是否引入外部产品设计，或查找某次研究结论和来源。报告是判断，不是当前实现的说明。
- [审查](review/index.md)：代码审查和审计报告，记录审查当天的问题清单、证据和修复建议。阅读时机：查某个审查问题（如 J 编号）的来由，或安排新的审查。报告是时点快照，不代表当前实现。
