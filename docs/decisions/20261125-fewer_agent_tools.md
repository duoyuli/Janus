# ADR：精简 Agent 的协作工具

- 状态：已采纳（accepted），记录于 2026-11-25。取代 [ADR：协作者认领文件，Janus 读回工作目录里的痕迹](20261113-claims_and_traces.md) 中协作者认领文件（`claim`）的部分，以及 [ADR：工作区值班派发独立的 Run](20261121-workspace_loops.md) 中值班自己起名（`set_name`）、转话（`note_dispatch`）和保存状态（`save_state`）的做法。
- 相关文档：[看板 MCP 工具](../interfaces/20261031-board_mcp.md)、[协作三方](../domains/20261119-collaboration_roles.md)、[团队结构](../domains/20261031-goal_mode.md#team_structure)、[工作区值班](../domains/20261121-loops.md)、[事件](../interfaces/20261031-events.md)、[ADR：Janus 只注入协作规则](20261125-collaboration_only_injection.md)、[ADR：协作者用工具交结果和提问](20261125-hand_in_with_tools.md)。

## 背景

`janus` MCP 服务器给负责人 10 个看板工具，给协作者 6 个，给值班 7 个，另加团队记忆的工具。每个工具都占 Agent 的上下文（工具说明里写着它的用法和限制），也是 Agent 每次要做选择的一个分支；几个工具做的事有重叠时，Agent 每次都要先分清该用哪一个。

用户要求逐个检查这些工具是否必要。对照约 41 个真实 Run 的工具调用记录，逐个看了每个工具的用法：

- 负责人的 `team` 和 `task_detail`、协作者的 `peers` 都是读看板，只差有没有指定一个子任务。
- `accept` 很少用到，而且只在一种情况下有意义：子任务没通过验收核对，负责人核对后接受它的结果。这是对子任务结论的一次修订，和 `amend` 是同一类裁决。
- `claim` 几乎没人调用：负责人在编排时就划好了 `owns`，协作者需要别的文件时更自然的做法是问负责人。
- `request_split` 总是和一个提问一起出现：提出拆分以后，协作者要停下等负责人调整编排，于是紧接着再调一次 `ask`。
- 值班的 `note_dispatch` 就是「同一件事再派一次」：值班想补充的，正是为同一个 key 再写一个目标。
- `dispatches` 查询的是 Janus 在值班醒来时就知道的事：这个值班派出的 Run 现在是什么状态。
- `set_name` 和 `save_state` 重复了已有的东西：名字是用户给值班的称呼，用户自己起更合适；值班的会话跨开工一直续用，会话本身就记得之前看过、做过什么，要长期留下的有团队记忆。

## 决策

原则是如无必要勿增实体：一个工具只在它做的事无法由已有工具、已有信息或 Agent 自己的机制完成时才存在。按这条原则合并或删除：

1. **`board`**：负责人的 `team` 和 `task_detail` 合并成 `janus.board`。不传 `task` 时返回协作者和所有子任务的状态，传 `task` 时返回这个子任务的全部信息。协作者的 `peers` 改名为 `janus.board`，行为不变。
2. **`amend` 兼管接受结果**：`accept` 并入 `amend`。对已完成的子任务，`amend` 照旧修订结论；对没通过验收核对的子任务（`task.failed`，`reason` 为 `verification`），`amend` 接受它的结果，`text` 是接受的理由。独立验证子任务（带 `verifies`）的判定和结论一律不能用 `amend` 改（`teamwork::amend_refused`）。
3. **拆分提议随提问一起交**：`request_split` 并入 `janus.ask`，作为可选的 `split` 参数（`reason`、可选 `completed`、`subtasks`，最多 8 项）。提议当场校验、立即写成 `task.split_requested` 送给负责人，问题照旧在这一轮结束时转交；提议不合法时整个提问被拒绝。验证子任务不能提拆分。
4. **去掉认领**：删除 `claim`、`task.claimed` 事件和 `teamwork::claim`。负责范围只由负责人在编排中的 `owns` 定下；协作者要改别的文件，先问负责人。越界和同时改同一个文件的检测、结果与痕迹的对账不变。
5. **值班的 `dispatch` 兼管转话**：`note_dispatch` 并入 `dispatch`。用派出过的 key 再调用 `dispatch` 从不新建 Run：那个 Run 是这个值班派出、正在进行的，`goal` 作为新情况转给它的负责人；还没开始工作的，告诉值班转告不了；别的值班或用户派出的，不转告；已经结束的，告诉值班它的结果。
6. **派出的 Run 写进醒来的提示**：删除 `dispatches`。每次醒来的提示在用户的提示之后，列出这个值班派出、还没结束的 Run（排队中、正在开始、进行中、中断了），再列出上次醒来后结束的 Run，最后是用户的回答。
7. **名字由用户起**：删除 `set_name` 和 `loop.named`。值班编辑器有一个可选的「名字」字段，接口的 `name` 可选；不填时用提示的第一行称呼。
8. **不另存值班的状态**：删除 `save_state`、`loop.state_saved` 和系统层里「你保存的状态」一节。值班跨开工续用同一个会话，之前的上下文由 Agent 自己保留和压缩；要长期记住的写进团队记忆（`remember`）。

合并后，负责人有 8 个看板工具（`board`、`orchestrate`、`wait`、`reply`、`retry`、`amend`、`cancel`、`ask_user`），协作者有 4 个（`board_post`、`board`、`submit`、`ask`），值班有 3 个（`dispatch`、`ask_user`、`next_wake`），各自另有团队记忆的 `recall` 和 `remember`。

## 取舍

- **一个工具按参数做两件事**：`board` 带不带 `task`、`amend` 对哪种子任务、`dispatch` 用的 key 是否派出过，都让一个工具有两种结果。代价是工具说明要把两种情况都写清楚；换来的是 Agent 不必在相近的工具之间选择，参数本身就说明了意图。
- **`amend` 不能改验证的结论**：原来 `amend` 可以修订独立验证子任务的结论。合并以后，同一个工具既能修订结论又能接受结果，如果对验证子任务也开放，负责人就能绕过一次没通过的独立验证。所以验证子任务的判定和结论都只能靠返工后重新验证来改变，或者用 `cancel` 写明理由。
- **协作者不能自己扩大负责范围**：认领让协作者不用等负责人就能拿下新的文件，但几乎没人用；去掉后，负责范围只有一个来源，越界检测和编排校验看的都是负责人写下的 `owns`。需要改别的文件时多一次提问，负责人也因此知道分工变了。
- **值班依赖一个长期会话**：不再有 `save_state`，值班记得什么取决于会话的上下文和 Agent 的压缩；会话因为加载失败而新开时，之前的上下文就丢了。派出的 Run 和用户的回答每次都写在醒来的提示里，丢不了；更长久的判断和事实要靠值班自己写进团队记忆。

## 后果

- `LeadTool` 去掉 `Team`、`TaskDetail`、`Accept`，加上 `Board`；`MemberTool` 去掉 `Claim`、`Peers`、`RequestSplit`，加上 `Board`；`LoopTool` 去掉 `Dispatches`、`NoteDispatch`、`SaveState`、`SetName`。负责人说明、协作者的约定和值班的系统层里「协作工具」一节随之改写；任务提示里负责的文件一段改为「只改这些；要改别的文件，先问负责人。」
- 删除事件类型 `task.claimed`、`loop.named`、`loop.state_saved`；早先写下的这些事件读成 `unknown`，审计视图照原文显示。恢复 Run 时不再把认领的路径并入 `owns`。
- `teamwork::accept_refused` 改名为 `amend_refused`，`board_request_split` 变为 `ask` 调用的 `propose_split`，`loop_note` 变为 `dispatched_again`。
- `POST /api/workspaces/{id}/loops` 和 `PUT /api/loops/{id}` 接受可选的 `name`；应用的值班编辑器加「名字」字段，值班页不再显示「它记下的状态」。
- 不扩大信任边界：没有新的下载来源、网络监听、用户目录读写或凭据存放变化。
