# ADR：工作没有时间和费用上限

- 状态：已采纳（accepted），记录于 2026-11-21。
- 相关文档：[Run 生命周期：时间限制](../domains/20261031-run_lifecycle.md#timeouts)、[Run 状态](../domains/20261031-run_lifecycle.md#run_statuses)、[停滞检测](../domains/20261031-run_lifecycle.md#watchdog)、[配置](../interfaces/20261031-configuration.md#compile_time)、[HTTP API：Run 操作](../interfaces/20261031-http_api.md#run_actions)。

## 背景

Run 原来有两道上限：6 小时墙钟，以及可选的已上报美元费用上限（`maxCostUsd` / `JANUS_RUN_COST_BUDGET_USD`）。任一道到达，Run 进入 `waiting_input`，不再启动新的子任务，等用户「追加预算」（`extendBudget`）。协作者的每次尝试另有 30 分钟上限，超时就取消这一轮、算作失败。

Janus 要演进为常驻的工作台：工作区的 Loop 按节奏醒来，把要改的事派成 Run，没有人一直看着。这时任何一道上限都会让 Run 停在「等你追加预算」或无故失败，而用户已经明确：token 和费用只是观测指标，不要任何时间上的限制。

## 决策

1. **去掉 Run 的墙钟和美元费用上限**：删除 `waiting_input` 状态、`extendBudget` 操作、`run.budget_exhausted` 和 `run.budget_extended` 事件、`maxCostUsd` 字段和 `JANUS_RUN_COST_BUDGET_USD` 配置。数据版本 18 把仍在 `waiting_input` 的 Run 改为 `interrupted`（用户从中断处继续），并从 Run 快照中删去 `costLimitUsd`。
2. **去掉单次尝试的 30 分钟上限**：一轮提示做多久都可以，`session/prompt` 不设超时。
3. **只保留防挂死的上限**：Agent 启动和打开会话（10 分钟）、ACP 控制请求（30 分钟）、发送打断等控制指令（15 秒）。它们判断的是进程和控制请求有没有响应，不限制工作时长。
4. **Token 参考值保留**：`maxTokens` / `JANUS_RUN_TOKEN_BUDGET` 只在应用里和已上报用量一起显示，从来不影响执行。

## 取舍

- **卡住由停滞检测发现，不由计时器结束**：会话 10 分钟没有任何动静或原地打转时，停滞检测报告给负责人（负责人自己卡住时报告给用户），由负责人或用户决定继续等、打断还是取消。代价是一个一直在调用工具、但输出每次都不同的死循环不会被认出，只能由人发现；原来 30 分钟的尝试上限能兜住它。
- **限制只来自真实约束**：机器资源（之后由跨 Run 的派发队列限制同时干活的 Run 数）、Agent 的限流、停滞和原地打转检测、人的等待、用户的停止。不按 token、费用或时长设预算。

## 后果

- 收件箱和 Run 页不再有「预算用完」卡片，工作区首页不再列「等你追加预算」。
- 早期 Run 的事件日志里仍有 `run.budget_*` 事件，应用不再据此改变 Run 的状态或生成待处理事项。
