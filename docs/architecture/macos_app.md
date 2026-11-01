# macOS 应用

本文说明 `macos/` 中的原生 macOS 应用：它和 Janus 服务的关系、源码结构、怎样从事件推导页面，以及它和 Web 工作台之间必须保持一致的地方。安装、打包和发布见[部署方案](../operations/deployment.md#macos_app)和 [CI 与发布](../operations/ci_release.md)，设计取舍见 [ADR：用原生 macOS 应用做展示层](../decisions/native_macos_app.md)。

## 形态

- SwiftUI 应用，Swift Package（`macos/Package.swift`），最低 macOS 14。只用系统框架，没有第三方依赖。
- 它是展示层：所有数据和操作都通过[HTTP API](../interfaces/http_api.md)读写，和 Web 工作台用同一套接口，编排器本身不变。
- 安装包 `Janus.app` 里同时放着应用（`Contents/MacOS/Janus`）和 Janus 服务（`Contents/MacOS/janus-server`，即 release 构建的 `janus`），下载后不需要编译 Rust。
- Web 工作台仍然可用：服务照常在 `/` 提供页面，应用菜单「帮助 → 在浏览器中打开网页版」直接打开它。

<a id="server_process"></a>
## 与 Janus 服务的关系

`Backend`（`Core/Backend.swift`）在启动时决定用哪个服务：

1. 读取 Janus 配置的地址：`JANUS_ADDR`，否则 `<Janus home>/config.toml` 的 `addr`，否则 `127.0.0.1:7878`，与服务自己的规则相同。
2. 这个地址上有 Janus 应答 `GET /api/meta`，并且它的 `dataDir` 就是这个 Janus home（例如 `just start` 启动的 launchd 服务，或 `just dev`）时，直接连接它，不另起服务。端口上是别的程序，或是另一个 Janus home 的 Janus，都不算。
3. `<Janus home>/janus.pid` 记录的进程还活着时，说明已有 Janus 在用这个 home，只是不在配置的地址上：从 `logs/janus.log` 最后一行「`http://<地址>  ·  home <目录>`」取得它的地址并连接。连不上时提示先退出那个 Janus，不再启动第二个（[单实例](../operations/deployment.md#single_instance)）。
4. 否则启动服务程序：优先用应用包内的 `janus-server`，其次是 `JANUS_SERVER_BIN`，最后是 `just install` 安装的 `<Janus home>/bin/janus`。30 秒内没有应答、或服务退出时，启动页显示日志末尾，提供「重试」和「打开日志目录」。

<a id="port_conflict"></a>
### 端口被占用

应用自己启动服务时，先按服务的方式（`SO_REUSEADDR`）试绑定配置的端口：

- 端口空闲就用它。
- 被占用时，依次试后面 20 个端口，都不行就由系统分配一个空闲端口。选中的地址通过 `JANUS_ADDR` 传给服务，服务写进看板 MCP 地址的也是它。
- 检查和服务真正绑定之间端口仍可能被抢走：服务因 `Address already in use` 退出时，换下一个端口重试，最多 3 次。
- 改用了其他端口时，侧栏底部的地址下显示原因，例如「端口 7878 被其他程序占用，改用 7879」。`config.toml` 不会被改写，下次启动仍先试配置的端口。

应用自己启动的服务：

- 标准输出和错误追加到 `<Janus home>/logs/janus.out.log`，和服务方式运行时一样。
- 从 Finder 打开的应用只有很短的 `PATH`，找不到 `node`、`git` 和 Agent CLI。应用用登录 shell（`$SHELL -ilc`）读取用户的 `PATH`，再补上 Homebrew、`~/.local/bin` 等常见位置，传给服务。
- 退出应用时停止服务（SIGTERM，3 秒后 SIGKILL）；被 `kill` 或 Ctrl-C 结束时也一样。还有进行中的 Run 时，退出前先确认，这些 Run 会中断，下次可以「从中断处继续」。
- 连接的是别人启动的服务（launchd 服务）时，退出应用不影响它。

同一个 Janus home 只能运行一个服务（[单实例](../operations/deployment.md#single_instance)），所以应用不会在已有服务时再启动一个。浏览器访问网页版时，用侧栏底部显示的地址，或菜单「帮助 → 在浏览器中打开网页版」。

## 源码结构

```text
macos/
├── Package.swift
├── Info.plist                 # 应用包的 Info.plist 模板，版本号由打包脚本填入
├── Sources/Janus/
│   ├── App.swift              # 入口、菜单命令、AppDelegate（启动和停止服务）、启动页
│   ├── Core/
│   │   ├── JSON.swift         # 动态 JSON 值，事件和设置对象都按字段读取
│   │   ├── API.swift          # HTTP 请求和 SSE 事件流
│   │   ├── Backend.swift      # 找到或启动 Janus 服务
│   │   ├── AppStore.swift     # Agents、角色、工作区、Run 列表、适配器、Skill 库，以及当前屏幕
│   │   ├── RunModel.swift     # 打开的 Run：事件、订阅、操作、回放、草稿
│   │   ├── Derive.swift       # derive / boardFacts 的 Swift 实现
│   │   ├── Format.swift       # 状态文字、事件图标与色调、时间和文本格式
│   │   ├── Setup.swift        # 会话控制项、权限模式标签、环境变量文本
│   │   └── Snapshots.swift    # 开发用：按命令保存窗口截图
│   └── UI/                    # 侧栏、工作区首页、Run 各页面、设置
├── Tests/JanusTests/          # Swift Testing 测试
└── scripts/                   # package.sh（打包）、make-icon.swift（图标）
```

<a id="event_derivation"></a>
## 由事件推导状态

和 Web 工作台一样，运行页不保存自己的业务状态。`derive(events)` 从头遍历事件，得到任务、Agent 状态、待处理事项、消息、交付结果；`boardFacts` 由此生成共享看板的待处理、结论、产物、交接和决定。两者是 `frontend/app.html` 中同名函数的逐条移植，规则见 [Web 工作台](frontend.md#event_derivation)。

这带来一条约束：新增或修改事件类型时，`frontend/app.html` 的 `derive` / `boardFacts` 和 `macos/Sources/Janus/Core/Derive.swift` 要一起改，见[事件参考](../interfaces/events.md)。

移植的差异只有一处：页面在早期 Run 的某些事件上（例如没有记录尝试的 `attempt.failed`）会抛出异常，Swift 版本跳过缺少前提的事件。

<a id="server_sync"></a>
## 与服务同步

| 场景 | 行为 |
| --- | --- |
| 打开 Run | `GET /api/runs/{id}` 取全部事件，再订阅 `/api/runs/{id}/stream?after=<最后一个 seq>` |
| 收到事件 | `seq` 必须正好是上一个加 1；出现缺口或收到 `lagged` 时重新读取整个 Run。一批事件到达后合并成一次推导（60 毫秒） |
| 事件流断开 | 2 秒后重新读取并订阅，只要这个 Run 还开着 |
| 发送操作 | 附加 `commandId` 和 `expectedSequence`；返回 409 时刷新一次再重试 |
| 中断的 Run | `resumeRun` 或 `message` 成功后重新连接 |
| Run 状态变化 | 立即刷新侧栏的 Run 列表；平时每 5 秒刷新一次 |
| Agent 检查、登录、适配器安装 | 在服务端后台进行，应用轮询直到结束 |

## 页面

| 位置 | 内容 |
| --- | --- |
| 侧栏 | 工作区、当前工作区的任务记录（带状态）、设置（Agents、角色、ACP 适配器） |
| 工作区首页 | 目标输入框（⌘↩ 交给主控）、团队、任务记录 |
| 工作区设置（表单） | 目录（系统的文件夹选择框）、启用的角色、主控 |
| 运行页 | 工具栏切换「工作台 / 审计与回放 / 团队与权限」；工作台有运行卡片、需要你处理、交付结果、任务图、共享看板和 Agent 状态 |
| 任务页 / Agent 页 | 会话、验收标准、执行轨迹、Prompt、分配、依赖、尝试、运行环境、产物 |
| 设置 | Agents（添加、编辑、检查、登录、删除）、角色（模型、指令、Skill、MCP、权限、会话选项）、ACP 适配器 |

产物在单独的窗口中查看，先读 `/api/file`，不在产物目录中的交付物直接读本机文件。

<a id="request_cards"></a>
## 请求卡片

需要用户处理的事项（主控的决定、协作者的提问、ACP 审批、表单或链接、预算、会话等待消息）都用 `RequestCard` 渲染，种类和 [Web 工作台的请求卡片](frontend.md#request_cards)一一对应。卡片出现在工作台的「需要你处理」，也内联在会话中，在任一位置处理都生效。

<a id="session_messages"></a>
## 会话中的消息

会话视图（`ChatCard`）和 Web 工作台的规则相同，见 [Web 工作台](frontend.md#session_messages)：

- 还没送达的消息在输入框上方的「待发送」区；插话、新一轮、排队后送达的消息显示在它真正进入会话的位置。
- 工具调用一行一个，点开看完整输入和输出；Janus 看板工具按用途显示，Agent 自己上报的同一次看板调用不重复显示；连续超过 4 次折叠，只露出最后两次。
- Agent 自主执行超过 20 秒没有新输出时，标题旁显示「N 秒没有新输出」，每 5 秒更新。
- 输入框按会话状态提供插话、接管、打断、交还托管、以当前结果完成、留言或「发送并继续」。

## 实现约束

- 视图中的局部状态用 `Local`（`SwiftUI.State` 的别名）声明：新 SDK 中 `@State` 是宏，宏插件只随 Xcode 提供，只装命令行工具时无法编译。别名展开为同一个属性包装器，两种环境都能构建。
- 带数字插值的界面文字用 `Text(verbatim:)` 或 `String(...)`，否则 SwiftUI 会按地区格式加千位分隔符（`#1,295`）。
- 浏览器存储换成 `UserDefaults`，只记住上次打开的工作区（`janus.ws`）。
