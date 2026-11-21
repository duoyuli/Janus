# Skill 路由的通行做法

- 日期：2026-11-20。
- 对象：Janus 的 Skill 路由 `janus-skills`（当天 `main` 的 `library::deliver`），对照 Agent Skills 规范的集成指南、Claude Code 和 Codex 的 Skill 列表、几个开源 Agent 框架、MCP 的 Skills 扩展，以及 2026 年关于 Skill 检索和路由的论文与实测。
- 信息来源：各家文档、GitHub issue、论文和社区实测，见文末「来源」。
- 范围限制：Claude Code 和 Codex 列表预算的具体数字来自文档和第三方测量，没有在 Janus 中逐项复现；论文的数字来自各自的语料和评测集，不能直接套到 Janus 的资源库上。

## 结论

Janus 用一个路由 Skill 代替逐个加载，方向和 magpie、开源 skill-router 一致，也符合规范「目录加按需读取」的三层加载。当时的实现有五个差距：路由 Skill 的描述只有名字，路由本身可能被 Agent 的列表预算挤掉，目录不随资源库变大而收敛，目录用的是截短到 240 个字符的描述，没有理会 `when_to_use` 和 `disable-model-invocation`。另外没有办法知道 Skill 有没有被用上。这些已经改进，采纳的设计见 [ADR：Skill 路由是一份按需查找的目录](../decisions/20261120-skill_catalog.md)，当前行为见[资源库：Skill 路由](../domains/20261116-resource_library.md#skill_router)。

## 外部现状

### Agent Skills 规范的集成指南

- 三层渐进加载：会话开始时加载目录（名字、描述、可选的 `SKILL.md` 路径，每个 Skill 约 50～100 token）；Skill 被选中时加载 `SKILL.md` 正文（建议少于 5000 token）；正文提到附属文件时再按需读取。
- 目录可以放在系统提示里（最简单，兼容任何能读文件的模型），也可以放在一个激活工具的描述里。目录旁边要有一小段行为指令，说明任务和描述相符时去读对应的 `SKILL.md`。
- 用户关掉的、权限不允许的、`disable-model-invocation` 的 Skill 应从目录中隐藏，免得模型白费一轮去加载用不了的 Skill。没有 Skill 时目录和指令都不出现。
- 激活工具的 `name` 参数建议做成枚举，防止模型编造 Skill 名。

### Claude Code

- 列表里每个 Skill 的 `description` 加 `when_to_use` 最多 1536 个字符，超出截断。
- 整个列表占上下文窗口的 1%（窗口未知时 8000 个字符）。超出时保留全部名字，从最少用的 Skill 开始去掉描述；用过的 Skill 排在前面。第三方测量：200K 窗口下内置 Skill 就能占满预算，项目 Skill 只剩名字。
- 只剩名字时，模型在 3 次试验中 0 次找到正确的 Skill（有描述时 7 次中 7 次）。
- `disable-model-invocation: true` 的 Skill 不进列表，只能由用户用 `/名字` 调用；`user-invocable: false` 的只给模型用；`skillOverrides` 可以把某个 Skill 设成只列名字或隐藏。
- 建议：描述把关键用途写在前面；有副作用的流程设成只能手动调用；`SKILL.md` 少于 500 行，细节放进附属文件；用 `/skill-doctor` 看每个 Skill 的开销和使用次数。

### Codex

- 目录包含每个 Skill 的名字、描述和 `SKILL.md` 路径，最多占上下文的 2%（窗口未知时 8000 个字符）。超出时先截短描述，再去掉描述，最后不列出一部分 Skill，并给出警告。`skills.max_context_tokens` 可以调高，上限 10000 token。
- 社区 issue 报告：119 个 Skill 时 103 个描述被截短；系统自带的 5 个 Skill 也占预算。
- 官方建议：描述简洁、范围清楚，把关键用途和触发词写在开头，这样截短后仍能匹配。`agents/openai.yaml` 的 `allow_implicit_invocation: false` 让 Skill 只能显式调用。

### 其他框架

- OpenCode 把 `available_skills` 目录放在 `skill` 工具的描述里，加载 Skill 就是一次工具调用，可以像其他工具一样设权限。
- Gemini CLI 把所有启用的 Skill 的名字和描述放进系统提示，`activate_skill` 的参数是 Skill 名的枚举。
- LangChain Deep Agents 的 `SkillsMiddleware` 把名字和描述放进系统提示，正文按需读取。
- magpie 的 Skill 路由和 Janus 当时的做法相同：一个 `magpie-skills`，描述在 1024 个字节内尽量多放各 Skill 描述的片段，正文列出描述和路径，没有检索。
- 开源的 skill-router（cangurel81）给几百个 Skill 建索引、分组和查重，教 Agent「想领域、搜两三个词、读路径、只打开选中的那个」，并标出短于 25 个字符、没法路由的描述。另一个 skill-router 报告 500 个以上 Skill 时每轮约 3 万 token，模型仍会挑最熟的而不是最对的。

### MCP 的 Skills 扩展（SEP-2640）

2026 年 9 月定稿：MCP 服务器用 `skills/list`、`skills/get` 提供 Skill，文件经 `resources/read` 读取，宿主按文件摘要做批准。服务端 SDK（Python、Rust、C#）已经支持，但截至本报告，Claude Code 和 Codex 作为宿主都还不向服务器请求 `skills/list`（Claude Code 只在内部做过原型，未确认）。

### 论文与实测

- **按需检索比全部列出省两个数量级**：690 个 Skill 全部列出每个任务约 4.7 万 token，按需检索约 560 token。混合排序（BM25 加向量）在不复述描述的查询上 hit@5 为 73.5%，纯关键词约 50%；用作者自己写的查询评估，会把准确率高估 21～44 个百分点。在排序之上加知识图谱反而差 11 个百分点（arXiv 2608.06196）。
- **正文比描述更有信号**：8 万个 Skill 的路由中，只给名字和描述比给正文低 37～44 个百分点；把正文提炼成更好的描述能补回一部分（SkillRouter，arXiv 2603.22455）。
- **部分露出可能更差**：13 个 Skill 的生产系统里，一部分直接加载、一部分只列描述时，直接加载的 Skill 在字面上和用户请求更像，把注意力从正确的那个拉走，路由反而比全露出或全不露出都差（arXiv 2608.20389）。
- **一层就够**：资源大到 Agent 自己翻不过来时，按需加载才关键，而且一层目录就够，多层目录的常驻描述反而增加上下文压力（arXiv 2607.17598）。
- **工具检索的对照**：Anthropic 的工具检索让 Opus 4.5 选工具的准确率从 79.5% 升到 88.1%、上下文少 85%；第三方在几千个工具上测得纯 BM25 只有 34%～64%，混合检索到 94%；Claude 会多搜几次弥补检索不准，平均每个请求 3.1 次。

## 对照 Janus

| 方面 | 外部做法 | Janus 当时 | 判断 |
| --- | --- | --- | --- |
| 常驻信号 | 描述写清做什么、何时用，关键词在前 | `janus-skills` 的描述只有名字列表 | 改：先写何时读它，再放名字和描述片段 |
| 列表预算 | 两种 Agent 都会截短或去掉描述 | `janus-skills` 的描述也受预算限制，协作者的说明里没提它 | 改：系统层写明目录的绝对路径 |
| 目录规模 | 少时列出，多时检索 | 正文列出全部描述，不随规模收敛 | 改：16 KB 以内逐行列出，超过时用 grep 搜 `catalog.txt` |
| 描述来源 | 完整描述加 `when_to_use`，上限 1536 | 用资源上截到 240 个字符的描述 | 改：从 `SKILL.md` 读完整内容 |
| 作者声明 | `disable-model-invocation` 从目录隐藏 | 不处理 | 改为列出并标注点名才用：资源库 Skill 不进 `/` 菜单，隐藏后就找不到 |
| 常用优先 | Claude Code 用过的在前 | 按名字 | 改：按读过的会话数排序 |
| 可观测 | `/skill-doctor` 看开销和使用次数 | 看不到 | 改：记 `skill.used`，资源库显示次数 |
| 描述质量 | 规范要求有描述，skill-router 标出太短的 | 只要求新建时有描述 | 改：资源库给出太短、没写何时用、太长、和别的太像四种提示 |
| 触发评估 | 用不复述描述的查询评估 | 没有 | 改：真实 Agent 的端到端测试加一个不复述描述的触发检查 |

## 不采纳

- **向量或混合检索**：要下载嵌入模型，扩大信任边界；几百个 Skill 以内，grep 加模型自己判断足够，模型检索不准时会再搜。资源库大到这一步时再评估。
- **知识图谱路由**：论文中比单纯排序还差。
- **在 `janus-tools` 里加 Skill 搜索工具**：排序更好，但 Skill 要依赖 MCP 路由，中文关键词还要分词；grep 零依赖，两种 Agent 都能用。
- **MCP Skills 扩展**：两种 Agent 都还不支持，支持后可以改由 `janus-tools` 原生提供 Skill。
- **少量 Skill 直接注入**：部分露出的研究结论支持只用一个入口，见 [ADR：资源库的每项资源都给所有数字员工，只经两个路由提供](../decisions/20261120-library_routed_only.md)。

## 来源

- Agent Skills：[How to add skills support to your agent](https://agentskills.io/integrate-skills)
- Anthropic：[Skill authoring best practices](https://platform.claude.com/docs/en/agents-and-tools/agent-skills/best-practices)、[Claude Code skills](https://code.claude.com/docs/en/skills)
- OpenAI：[Codex: build skills](https://learn.chatgpt.com/docs/build-skills)、[openai/codex #24299](https://github.com/openai/codex/issues/24299)
- 实测：[With 160 skills installed, Claude Code kept 20 descriptions](https://dev.to/rulestack/with-160-skills-installed-claude-code-kept-20-descriptions-and-the-model-found-0-of-3-skills-that-10j2)、[Anthropic Tool Search Test: 4,000 Tools](https://www.arcade.dev/blog/anthropic-tool-search-4000-tools-test/)、[Tool Discovery for AI Agents](https://www.getmaxim.ai/articles/tool-discovery-for-ai-agents/)、[Picking the right tool for an LLM](https://kachar.dev/blog/jev-picks-the-right-tool-for-an-llm)
- 论文：[Comparative Approaches to Agent Retrieval over Large Skill Libraries](https://arxiv.org/abs/2608.06196)、[Representation Affects Retrieval](https://arxiv.org/abs/2608.20389)、[SkillRouter](https://arxiv.org/abs/2603.22455)、[The Router Within](https://arxiv.org/abs/2609.15982)、[Is Progressive Disclosure All You Need for Long-Context Agents?](https://arxiv.org/abs/2607.17598)
- 开源：[magpie skill router](https://github.com/yetone/magpie/blob/main/docs/subsystems/skill-router.md)、[cangurel81/skill-router](https://github.com/cangurel81/skill-router)、[Gemini CLI activate_skill](https://geminicli.com/docs/tools/activate-skill/)、[OpenCode skills](https://www.skillsboard.sh/opencode-skills)、[Deep Agents skills](https://docs.langchain.com/oss/python/deepagents/skills)
- MCP：[SEP-2640 Skills Extension](https://modelcontextprotocol.io/seps/2640-skills-extension)、[zeke/skills-over-mcp](https://github.com/zeke/skills-over-mcp)
