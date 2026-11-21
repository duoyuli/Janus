# 默认数字员工

这个目录里每个 `.md` 文件是一名默认数字员工，编译时打进 Janus（`backend/src/agents/roles.rs` 的 `ROLE_FILES`），运行时不读这个目录。首次启动的 Janus 按 `ROLE_FILES` 的顺序写入它们。

## 文件格式

```markdown
---
id: inner-grill
name: 老友
desc: 像老朋友一样陪用户把一件事聊透：……
color: "#be185d"
init: 老友
lead: false
---

这里是数字员工的指令，原样作为角色指令。
```

| 字段 | 含义 |
| --- | --- |
| `id` | 数字员工的 id，和文件名相同（`inner-grill.md`），只能用小写字母、数字和 `-`。发布后不要改：用户数据按它认出默认数字员工 |
| `name` | 显示的名字 |
| `desc` | 职责，一行。负责人只凭名字和职责认识协作者，写成「什么样的子任务适合派给它」 |
| `color` | 头像颜色，带引号，例如 `"#be185d"` |
| `init` | 头像上的字 |
| `lead` | 能否担任负责人：`true` 或 `false` |

`---` 之后的正文是指令，开头和结尾的空行会去掉。

## 改了之后会怎样

- **改职责或指令**：用户升级后，没改过这一项的数字员工自动换成新文本；改过的保留用户的文本，编辑器里提示有新版，由用户选择采用或保留。不需要写迁移。
- **新增一名**：新建文件，把它加进 `ROLE_FILES`；再在 `backend/src/migrations.rs` 的 `MIGRATIONS` 末尾加一条迁移（照数据版本 15 的「老友」，用 `add_seed`），老用户升级时才会有它，然后运行 `just data-shape`。
- **删除一名或改 `id`**：老用户数据里还有它，要另写迁移处理，先和维护者商量。

改完运行 `just test`：`every_role_file_is_built_in` 检查每个文件都已加进 `ROLE_FILES`、格式正确、`id` 和文件名一致。规则见 [默认角色](../../docs/domains/20261031-agents_and_roles.md#product_launch_roles)。
