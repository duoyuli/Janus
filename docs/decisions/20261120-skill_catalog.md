# ADR：Skill 路由是一份按需查找的目录

- 状态：已采纳（accepted），记录于 2026-11-20。
- 修订：[ADR：资源库的每项资源都给所有数字员工，只经两个路由提供](20261120-library_routed_only.md) 的第 3 条「`janus-skills` 列出每个 Skill 的描述和 `SKILL.md` 路径」：现在它是一份按使用次数排序、太大时改为按关键词搜索的目录，系统层另写明它的位置。Skill 只经 `janus-skills` 的决定不变。
- 相关文档：[资源库：Skill 路由](../domains/20261116-resource_library.md#skill_router)、[Skill 的使用记录](../domains/20261116-resource_library.md#skill_uses)、[Skill 的改进提示](../domains/20261116-resource_library.md#skill_hints)、[调研：Skill 路由的通行做法](../research/20261120-skill_routing_practices.md)。

## 背景

资源库的 Skill 都经 `janus-skills` 交给 Agent。它的描述只有名字列表，正文列出每个 Skill 截到 240 个字符的描述和路径。调研（见上面的调研报告）发现：

- Agent 一直看得到的只有 Skill 的描述，只剩名字时模型几乎找不到正确的 Skill。
- Claude Code 和 Codex 的 Skill 列表都有预算，超出时去掉描述。`janus-skills` 自己的描述也可能被去掉，而协作者的说明里没提它。
- 正文列出全部描述，资源库到几百个 Skill 时，读一次目录就是几万 token，和全部加载差不多。
- 「何时使用」常写在描述末尾，截到 240 个字符时正好被截掉。
- 没有办法知道一个 Skill 有没有被用上，也没有办法发现写得不好的描述。

## 决策

1. **系统层指路**：成员有资源库 Skill 时，负责人、协作者和对话的系统层都写一句话，给出 `janus-skills` 的 `SKILL.md` 的绝对路径，开始任务时先读它。只指向目录，不列出具体的 Skill。
2. **路由的描述先写何时用**：开头写明「开始一项任务时先读它」，再列「名字（描述片段）」，片段取能让全部 Skill 放下的最长长度，总长不超过 1000 个字节。
3. **目录分两档**：每个 Skill 一行（完整描述加 `when_to_use`，最多 1536 个字符），加起来不超过 16 KB 时逐行列在正文；超过时正文只列名字，描述放进 `catalog.txt`，由 Agent 用 grep 按关键词搜索。
4. **常用的在前**：Janus 从工具调用里认出 Agent 读了哪个 Skill 的 `SKILL.md`，记一条 `skill.used`；新 Run 的目录按读过的会话数排序，描述放不下时先保住常用的。
5. **作者声明的字段**：`when_to_use` 并入目录；`disable-model-invocation: true` 的 Skill 照样列出，标注只在用户或负责人点名时使用。
6. **资源库显示使用次数和改进提示**：每个 Skill 带读过它的会话数，以及描述太短、没写何时用、太长被截、和别的 Skill 太像四类提示。提示只是提醒，不拒绝保存。服务端按类别返回，应用按界面语言写成文字。

## 取舍

- **系统层多一句话**：[ADR：资源库统一管理 Skill 和 MCP，在会话启动时注入](20261116-launch_time_resources.md) 不在提示词里暴露资源；这句话只给目录的位置，不列具体的 Skill，系统层不随资源库增长。换来的是 Agent 列表预算不够时仍能找到资源库的 Skill。
- **grep 而不是检索工具或向量检索**：零依赖，两种 Agent 都能用，不扩大信任边界。排序不如混合检索，模型可以多搜几次弥补。资源库大到 grep 明显不够时再评估。
- **点名才用的 Skill 列出而不是隐藏**：规范建议从目录中隐藏，但资源库 Skill 不进 Agent 的 `/` 菜单，隐藏后用户在目标里点名也找不到它。标注后由 Agent 遵守。
- **按读过的会话数排序**：只反映「被读过」，不反映「用得对」；新加的 Skill 一开始排在后面，名字和片段仍在描述里，Agent 也会先读完整目录或搜索。
- **提示用启发式规则**：「没写何时用」靠关键词判断，「很像」靠词的重合度，会有误报和漏报；所以只提示、不拦截。

## 后果

- 新增 `library::skill_router`（目录）和 `library::quality`（提示）；`Db::skill_uses` 从事件里数使用次数，不新增表和字段，没有数据迁移。
- 新事件 `skill.used`；`session.prompt` 多一个 `channel`：`skills`；`GET /api/library` 的 Skill 多 `uses`、`hints`，顶层多 `skillRouter.catalog`。
- 应用的 Skill 路由说明从三步改为四步（加「先查目录」），按目录顺序显示，资源库行显示使用情况和提示。
- 替身 Agent 加 `skill:<名字>`，产品端到端测试加 `skills` 场景，真实 Agent 的端到端测试加一个不复述描述的触发检查。
