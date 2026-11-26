# ADR：不做从文件导入数字员工

- 状态：已采纳（accepted），记录于 2026-11-26。取代 [ADR：数字员工按分组管理，用文件导入导出](20261125-role_files.md) 的第 3 条（导入）和它带来的取舍、接口；取代 [ADR：招聘顾问在对话里用工具交出数字员工](20261126-propose_employees_in_chat.md) 中「确认用导入表单」「从文件导入仍在设置里」的说法。
- 相关文档：[数字员工文件](../domains/20261031-agents_and_roles.md#role_files)、[对话里提议的数字员工](../domains/20261120-chats.md#proposed_employees)、[安全边界](../operations/20261031-security.md#role_files)。

## 背景

「设置 › 数字员工」的「导入…」从设置浮层上点击没有反应：导入表单挂在主窗口上，设置浮层打开时出不来。用户看到后决定不做导入功能。招聘顾问已经改为在对话里用工具交出数字员工、用户确认后创建，新建数字员工不再需要文件这条路。

## 决策

1. 删除从文件夹或文件导入数字员工：设置页的「导入…」、扫描和导入接口（`POST /api/roles/import/scan`、`POST /api/roles/import`）、读取用户选的目录（层数、文件数、大小、旁边的 PNG 头像）和 Claude Code subagent 文件的兼容。
2. 数字员工文件的格式保留：默认数字员工（`backend/roles/`）用它，「导出」也写它。读文件（`RoleFile::parse`）只读默认数字员工。
3. 对话里提议的数字员工用自己的确认表单（`ProposalSheetView`），按编号（`n`）勾选，`POST /api/runs/{id}/proposal` 的 body 是 `{proposal, picks, connection}`。创建和导入共用过的部分（核对 `RoleFile::checked`、创建 `make_roles`）留给提议用。

## 取舍

- **删而不是修**：修好要让表单在设置浮层之上弹出；但用户不需要这条路，留着就要维护读用户目录这一块信任边界和 subagent 兼容。导出仍然保留，作为备份和查看的方式。
- **信任边界变小**：服务不再读用户选的目录，安全边界里对应的限制一并删除。

## 后果

- 删除 `scan`、`import_roles` 和相应的应用代码、测试与端到端场景；`roles` 场景改用编辑器接口建数字员工。
- 导出的文件没有地方再读回；以后若要「从文件恢复」，作为新功能重新评估。
