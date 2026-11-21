# 研究文档索引

> 上级索引：[工程文档](../index.md)

## 范围

本分类记录外部产品、论文和社区进展的定期观察，以及从中得出的、给本项目的借鉴建议。报告描述的是外部事实和判断，不是 Janus 当前实现；当前行为以其余分类为准。

## 文档列表

- [2026-11-10 多 Agent 协作与 Agent Team 进展](20261110-multi_agent_collaboration.md)：Anthropic、OpenAI、Microsoft、Nous Research、PagerDuty、CrowdStrike 的本周进展，以及按 P0 / P1 / P2 分级的借鉴建议。阅读时机：评估是否引入显式中断、成本账本、证据分级、安全审查模板或紧急停止。
- [2026-11-11 X 直接抓取补充](20261111-multi_agent_collaboration.md)：用 OpenCLI 读 X 后补充的 Antigravity `/swarm`、Anthropic 1,000-Agent 实验、并行小队论文和 Claude Code 动态工作流；修正上一份的结论，并新增验证者模式等建议。阅读时机：评估公开看板的规模化验证、验证者角色、子任务拆分或可保存的编排模板。
- [2026-11-12 多 Agent 协作与 Agent Team 进展](20261112-multi_agent_collaboration.md)：1,000 个 Agent 的启动瓶颈与 Git 索引锁经验、多 Agent 成本仍缺可见性、规则要变成机制；给出只读 Git 探测、启动并发上限和 CI 门禁的建议。阅读时机：评估会话启动并发、只读 Git 探测或 CI 合并检查。
- [2026-11-16 magpie 的资源库](20261116-magpie_library.md)：magpie 怎样统一管理指令、MCP、Skill 等资源并写进各 Agent 的配置，各 Agent 在 ACP 启动时能接收什么，以及 Janus 采纳和不采纳的部分。阅读时机：扩展资源库的资源类型，或评估 MCP 检测、MCP 路由、子代理。
- [2026-11-16 Agent Team 市场与 Grok Bot 研究](20261116-agent_team_market_landscape.md)：Grok Bot、Claude Code Agent Teams、OpenAI hosted multi-agent、Antigravity、GitHub Agent HQ 和 Microsoft Agent Framework 的对比，以及给 Janus 的 P0 / P1 / P2 演进建议。阅读时机：评估 Agent Team 产品能力、Grok Bot 借鉴项、验证者、成本预算、并发、编排模板或主动运行。
- [2026-11-17 动态数字员工与资源装配](20261117-dynamic_digital_workers.md)：负责人按任务创建 Run 内临时成员并从资源库装配 Skill / MCP 的可行性、资源绑定授权、失败处理和分阶段建议。阅读时机：评估动态组队、临时成员、任务级资源附加、资源选择或成员生命周期。
- [2026-11-20 MCP 路由的通行做法](20261120-mcp_router_practices.md)：对照 MCP 规范、MCP 网关和 Claude Code、Codex 的工具搜索检查 `janus-tools`，采纳分页、取消、版本协商、未知工具错误码、命名空间和说明长度等改进，不做路由自己的工具搜索。采纳的部分落在[资源库：MCP 路由](../domains/20261116-resource_library.md#mcp_router)。阅读时机：评估工具搜索、MCP 协议升级、工具定义审核，或核实 Agent 是否延迟加载路由的工具。
- [2026-11-20 Skill 路由的通行做法](20261120-skill_routing_practices.md)：对照 Agent Skills 规范、Claude Code 和 Codex 的 Skill 列表预算、开源框架、MCP Skills 扩展和 Skill 检索论文检查 `janus-skills`，采纳系统层指路、先写何时用的描述、两档目录、常用优先、使用记录和描述提示，不做向量检索和 Skill 搜索工具。采纳的部分见 [ADR：Skill 路由是一份按需查找的目录](../decisions/20261120-skill_catalog.md)。阅读时机：评估 Skill 检索、资源库规模变大后的目录方式，或核实 Agent 的 Skill 列表预算。
- [2026-11-20 Vercel Board 与 Janus 的工作台演进](20261120-vercel_board_workbench.md)：对照两者的身份、事件、任务与外部同步机制，给出工作台的目标形态和分阶段建议。阅读时机：设计服务端收件箱、GitHub issue 入口、PR 交付或手机访问时。
- [2026-11-20 Vercel Board 模板源码研究](20261120-vercel_board_templates.md)：沿 factory 和 swarm 模板追踪从 issue 到 PR 的分流、执行、审查和跟进。阅读时机：核对凭据隔离、仓库权限、自动化预算和人工决策位置时。

## 约定

- 文件名用 `YYYYMMDD-<主题>.md`，日期是报告日。定期观察覆盖上一次报告到当天，专题研究按主题单独成篇。
- 每份报告开头写明覆盖窗口、信息来源和抓取限制。
- 结论尽量落到具体文档章节或代码位置；没有核实的内容标注「未确认」，不写成事实。
