# ADR：Claude Code 的自定义 API 用环境变量设置

- 状态：部分被取代（superseded）：编辑时私有变量只显示名字，已被 [ADR：Agent 的环境变量在编辑器里全部显示](20261121-agent_env_visible.md) 取代；其余仍然有效。记录于 2026-11-20。
- 相关文档：[Agent 环境变量](../domains/20261031-agents_and_roles.md#agent_env)、[凭据保存](../operations/20261031-security.md#secrets)、[数据升级](../architecture/20261031-data_model.md#migrations)。

## 背景

Anthropic 兼容网关可以要求 `ANTHROPIC_AUTH_TOKEN`，并通过环境变量指定地址和各档模型。原先 Claude 的 API Key 表单要求单独填写 `ANTHROPIC_API_KEY`，高级设置里的变量又会原样保存在 SQLite 中。

## 决策

Claude 的「自定义 API」（id 仍是 `apikey`，与 Codex 的同名方式一致）改为一个多行环境变量输入框，接受 `KEY=value` 和 `export KEY=value`，不再有单独的 API Key 字段。catalog 为这种方式提供模板，应用的「填入模板」把模板中还没有填写的变量加到末尾，已填写的不变。配置检查通过实际会话验证认证。复制本机配置和订阅登录仍有各自的入口。

Claude 手动配置的变量中，已知的模型参数和普通服务地址可以回显。其他变量的值写在该 Agent 已有的 `0600` 凭据文件中，设置对象只保存变量名。升级时将旧 API Key 作为该 Agent 的 `ANTHROPIC_API_KEY` 继续使用；认证方式的 id 不变，所以不需要改写，也不为发送旧表单的客户端保留兼容逻辑（应用和服务的版本握手保证两边同一版本）。

## 取舍和后果

新网关可以直接使用自己的凭据变量名。编辑已有连接时，只保留变量名就沿用私有值，填写新值可以替换它，删除整行可以移除它。用户可以查看和修改常用的地址和模型，也可以整段替换配置。
