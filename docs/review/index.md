# 审查文档索引

> 上级索引：[工程文档](../index.md)

## 范围

本分类保存代码审查和审计报告。报告记录审查当天代码快照的结论，写成后不随代码更新，不代表当前实现；当前行为以其余分类为准。问题的跟进结果记录在 [经验教训](../../LESSONS.md#cleanup_log)，接受的风险写在[安全边界](../operations/20261031-security.md)。

## 文档列表

- [2026-11-18 Antigravity 协作架构审查与优化方案](agy-20261118-multi_agent_collaboration.md)：对 commit `57a35dd` 编排器多 Agent 协作链路的架构审查与业内实践对照（Antigravity `agy`），列出 MA-01 至 MA-07（角色并发槽位与全局并发上限、`session/load` 原会话返工热恢复、协作者资源可见性与 `wait` 脉搏、结构化 `handoff` 交接、负责人编辑痕迹与跨模型聚焦式验证、`request_split` 拆分提议、交替死循环检测与成本闸门）及 DOC-01 至 DOC-02 文档漂移，并给出三阶段路线图。阅读时机：规划多 Agent 协作吞吐、会话复用、看板工具、上下游交接或独立验证演进时。
- [2026-10-31 代码审查](20261031-code_review.md)：对 `backend/`、当时的前端、`fake_acp.py` 和 `justfile` 的审查，列出 J-01 至 J-11 共 11 个问题（看板 MCP 身份绑定、凭据进入 SQLite、`commandId` 去重、事件持久化失败后仍广播、质量门禁等）、修复顺序和建议的验收用例。阅读时机：查某个 J 编号问题的来由和当时的证据，或安排下一次审查时对照范围。

## 约定

- 文件名默认用 `YYYYMMDD-<主题>.md`，日期是审查日；按用户要求带 Agent 前缀做横向对照的报告采用 `<agent>-YYYYMMDD-<主题>.md`（如 `agy-20261118-multi_agent_collaboration.md`、`codex-20261118-agent_collaboration_log_analysis.md`）。
- 每份报告开头写明审查日期、对象、方式和范围限制，问题带稳定编号。
- 报告写成后不回改正文。高影响问题要么修复，要么在安全边界中写成明确接受的风险（合并前检查第 6 条）；处理结果写进 `LESSONS.md` 的清理记录。
