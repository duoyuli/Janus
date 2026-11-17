# ADR：按 models.dev 的价格估算用量

- 状态：已采纳（accepted），记录于 2026-11-15。
- 相关文档：[用量口径](../interfaces/20261031-events.md#usage_accounting)、[用量账单](../interfaces/20261031-http_api.md#usage_bill)、[模型价格](../interfaces/20261031-http_api.md#prices)、[安全边界](../operations/20261031-security.md#model_prices)、[目录规范](../operations/20261031-deployment.md#layout)。

## 背景

用量账本只有 token 和 Agent 自己上报的费用。Codex 和多数 Agent 不报费用，Claude Code 报的是会话累计的估算，所以账单上的金额大多是「未上报」，用户无法比较一段时间里各 Agent、各模型花了多少钱。按 token 折算需要每个模型的价格；Janus 自己维护价格表会很快过时。models.dev 是公开的模型目录，按提供方列出每个模型的价格（美元 / 百万 token，含缓存读写），提供一个完整的 JSON（`https://models.dev/api.json`）。

## 决策

1. Janus 从 `https://models.dev/api.json` 同步价格：服务启动一分钟后开始，每小时检查一次，表超过一天就重新下载；设置的「用量账单」可以立即同步（`POST /api/prices/sync`）。
2. 下载用系统的 `curl`，只允许 HTTPS（`--proto =https`），限时 120 秒、限 64 MB；内容只当数据解析，不执行任何部分。解析失败或没有读到任何价格时保留原来的表。
3. 每个模型只留一个价格，存到 `~/.janus/prices/models.json`：模型系列的厂商自己列的价格优先（`claude` 取 Anthropic，`gpt`、`o3` 取 OpenAI，`deepseek` 取 DeepSeek 等）；厂商没有列出的模型取多数提供方的价格，忽略免费的列表项。按模型名的最后一段、小写、去掉 `[1m]` 之类的后缀查找。
4. 估算只出现在账单里，标「按 models.dev API 价格折算 · 非账单」，和 Agent 上报的费用分开显示；不写进事件，不影响预算、调度或交付。没有价格的模型列出轮数，不计入估算。长上下文的加价不计。

## 取舍

- **新的下载来源**：信任边界多了 models.dev。内容只是价格数据，最坏情况是账单上的估算不对；不会执行代码，也不会改变 Agent 的行为。
- **估算不是账单**：订阅、批量折扣、长上下文加价、各家缓存计费细节都不在里面。界面和文档都写明这是估算。
- **按系列认厂商**：主机商和转售商也会列出别人的模型（NVIDIA 免费列出 DeepSeek），只看「哪个厂商的列表里有」会选错价格；按模型名的系列前缀认定厂商，新系列需要在 `prices.rs` 的 `LABS` 中补上，没补时退回多数提供方的价格。
- **用 `curl` 而不加 HTTP 客户端依赖**：macOS 自带 `curl`，后端不必为一次下载引入 TLS 依赖。

## 后果

- 新增 `backend/src/prices.rs`、`~/.janus/prices/`、`GET /api/prices` 和 `POST /api/prices/sync`。
- 时间段账单（`GET /api/usage`）在有价格表时带 `estimate`，每个 Agent 和模型带各自的估算；任务账单由应用按 `GET /api/prices` 返回的价格估算。
- 没有网络或 models.dev 不可用时，账单只显示 token 和 Agent 上报的费用。
