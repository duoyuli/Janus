# Project Doc 初始化备忘录

> 在初始搭建期间保留本文件。每次完成一个工作单元后立即更新，所有完成标准达成后删除。

## 当前状态

- 仓库：<code>{{repository_root}}</code>
- 文档根目录：<code>{{doc_root}}</code>
- 主语言：<code>{{primary_language}}</code>
- 整体状态：<code>{{planned|in_progress|blocked}}</code>
- 当前处理项：<code>{{current_item_or_none}}</code>
- 下一步：{{next_action}}
- 最后更新时间：<code>{{timestamp_with_timezone}}</code>

## 结构决策

- {{decision_and_reason}}

## 索引规划

| 路径 | 分类 | 范围与阅读时机 | 权威代码源 | 遗留处置 | 状态 | 下一步 |
| --- | --- | --- | --- | --- | --- | --- |
| <code>{{document_path}}</code> | <code>{{category}}</code> | {{scope_and_read_when}} | <code>{{source_paths}}</code> | <code>{{adopt|update|merge|split|new}}</code> | <code>{{planned|in_progress|verified|blocked}}</code> | {{item_next_action}} |

## 锚点规划

| 文档章节 | 稳定 ID | 代码入口点 | 状态 |
| --- | --- | --- | --- |
| <code>{{document_path}}#{{section_id}}</code> | <code>{{section_id}}</code> | <code>{{code_path_or_symbol}}</code> | <code>{{planned|in_progress|verified|blocked}}</code> |

## 既有文档

| 原始路径 | 处置方式 | 目标路径或原因 | 引用修复状态 |
| --- | --- | --- | --- |
| <code>{{legacy_path}}</code> | <code>{{adopt|update|merge|split|exclude}}</code> | {{target_or_reason}} | <code>{{planned|in_progress|verified|blocked}}</code> |

## Skill gate

| 智能体入口点 | 操作 | 状态 |
| --- | --- | --- |
| <code>{{instruction_path}}</code> | <code>{{create|update|unchanged}}</code> | <code>{{planned|in_progress|verified|blocked}}</code> |

## 阻塞项与待确认问题

- <code>{{blocked_or_none}}</code>

## 验证记录

| 时间 | 范围 | 检查项 | 结果 |
| --- | --- | --- | --- |
| <code>{{timestamp}}</code> | <code>{{scope}}</code> | {{validation}} | {{result}} |

## 交接说明

- 已验证：{{verified_summary}}
- 需保留的已完成工作：{{completed_work_to_preserve}}
- 恢复入口：{{exact_file_symbol_or_command_to_inspect_next}}
- 当前风险：{{known_risk_or_none}}
