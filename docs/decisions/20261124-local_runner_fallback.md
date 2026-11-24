# ADR：GitHub 的 macOS 运行器排不到时，改在维护者的本机运行器上跑

- 状态：已采纳（accepted），记录于 2026-11-24。
- 相关文档：[CI 与发布：本机运行器](../operations/20261101-ci_release.md#local_runner)、[安全边界：本机运行器](../operations/20261031-security.md#local_runner)。

## 背景

CI 的应用测试和 Release 的打包都要 macOS arm64 运行器。GitHub 托管的这类机器容量不足时，任务一直排队，约 15 分钟后被 GitHub 取消（「The job was not acquired by Runner of type hosted even after multiple attempts」）。2026-11-24 发布 2026.11.24.4 时，同一个提交的应用测试任务连续两次这样被取消，打好的安装包卡在「等 CI 通过」上发不出去。测试本身在本机早就通过了，缺的只是一台 GitHub 认可的机器。

用户的要求：5 分钟内排不到 GitHub 的运行器，就改到本机去跑。

## 决策

1. **在维护者的 Mac 上注册一个自托管运行器**（标签 `janus-local`），以登录后启动的服务常驻。安装方式、版本和校验见[本机运行器](../operations/20261101-ci_release.md#local_runner)。
2. **每个 macOS 任务有两条腿，出自同一份定义**：`hosted` 在 GitHub 的 `macos-15` 上，`local` 在本机运行器上。`local` 立刻开始，先给 `hosted` 5 分钟；托管机器接单了就让出，没接单就接手（步骤「Take over」）。`hosted` 的机器来得晚时，看到本机已经接手就让出。判断在 `.github/scripts/claim-runner.sh`，两条工作流共用。
3. **只有推送触发的运行有 `local` 这条腿**：CI 里只有推送到 `main`，Release 里是推送标签和手动运行。Pull Request 的运行只有 `hosted`。
4. **CI 的结论看任务「CI result」**：编排器的检查通过，并且有一条腿通过了应用测试。Release 的「等 CI」等的是它，不再看整次运行的结论。
5. **外部贡献者的工作流一律先审批**：仓库的「Fork pull request workflows from outside collaborators」设为「Require approval for all external contributors」。Agent 不批准这类运行，写进 `AGENTS.md` 的约定。

## 取舍

- **公开仓库上的自托管运行器**：GitHub 的文档不建议这样做，因为 Pull Request 可以改工作流文件，把任务指到自托管运行器上，在那台机器上执行任意代码。这里接受这个风险，靠第 5 条挡住：外部贡献者的运行不经批准不会开始。能推送到 `main` 和标签的只有维护者自己，他们本来就能在这台机器上运行任何代码。
- **常驻服务，而不是按需启动的一次性运行器**：GitHub 有只跑一个任务就注销的 just-in-time 运行器，暴露面更小，但要有一个本机进程盯着队列、在任务饿了 5 分钟后启动它，等于自己写一个调度器。先用标准的常驻服务；外部 PR 变多、审批成为负担时再换。
- **两条腿同时排队，而不是先探测再选机器**：任务排上队之后不能换运行器，也不能单独取消一个任务，所以「先试托管、不行再换」只能是两个任务。`local` 那条腿自己等 5 分钟，不另开一个等待任务。托管机器紧张但没有断供时，本机运行器每次推送会被占住最多 5 分钟，什么也不做。
- **被饿死的 `hosted` 腿会让整次运行显示失败**：GitHub 取消它之后，运行的结论是失败，即使 `local` 已经通过。没有办法取消单个任务，也不能确定 `continue-on-error` 管不管这种取消，所以另设「CI result」作为唯一的判定。Release 的运行在这种情况下也会显示失败，而安装包已经由 `local` 发布：以 Release 页面和 `local` 那条腿为准。
- **本机打出的安装包和 GitHub 上打出的不完全一样**：本机用的是这台机器上的 Xcode 命令行工具和 Rust（写这份 ADR 时是 macOS 27 的 SDK、Swift 6.4、Rust 1.99），GitHub 的 `macos-15` 用它镜像里的版本。最低系统版本、签名证书和打包脚本相同。发布前的 `just e2e --packaged --real` 验证的本来就是本机打出的包，所以本机这条路验证得更充分；但同一个版本号的安装包出自哪条腿，要看那次运行。
- **不在本机运行器上装 Rust 工具链**：`dtolnay/rust-toolchain` 会改掉维护者自己的默认工具链，本机这条腿跳过它，用机器上已有的。

## 后果

- 托管的 macOS 运行器排不到时，CI 和发布不再卡住；本机关机、没登录或运行器服务没在跑时，退回原来的行为（等托管机器，排不到就失败）。
- 维护者的机器多了一个常驻进程，它执行这个仓库的工作流：安全边界多了一节，见[安全边界：本机运行器](../operations/20261031-security.md#local_runner)。
- Release 的签名步骤在本机会改用户的钥匙串搜索列表，工作流最后一步把它恢复原样并删掉临时钥匙串。
- 公开的任务日志里会出现本机运行器的工作目录，所以它装在不带用户名的 `/Users/Shared/janus-actions-runner`。
