# CI 与发布

本文说明 GitHub Actions 中的两条工作流：每次推送运行的测试，以及推送版本标签时构建并发布 macOS 安装包。打包脚本本身见[部署方案](20261031-deployment.md#macos_app)。

## 导航

- [持续集成](#ci)：修改 CI 检查时阅读。
- [发布](#release)：发布新版本时阅读。
- [版本号](#versioning)：确定新版本号时阅读。
- [更新部分](#update_parts)：修改增量更新包时阅读。
- [签名](#signing)：修改签名证书和流程时阅读。
- [本机运行器](#local_runner)：macOS 任务排不到 GitHub 的机器、安装或撤掉本机运行器时阅读。

<a id="ci"></a>
## 持续集成

`.github/workflows/ci.yml` 在推送到 `main` 和每个 Pull Request 时运行，同一分支的新推送会取消还在跑的旧任务：

| 任务 | 运行环境 | 内容 |
| --- | --- | --- |
| Orchestrator | `ubuntu-latest` | `cargo fmt --check`（按 `backend/rustfmt.toml`，格式不一致就失败）、`cargo clippy --all-targets --locked -- -D warnings`（没有诊断才通过），然后 `cargo test --locked`（Rust 编排器的全部测试，含 `flow_tests`） |
| macOS app (hosted) / (local) | `macos-15` / 本机运行器 | `swift build --build-tests`、`swift test --skip-build`（应用的构建和 Swift Testing 测试，只编译一次）。两条腿只有一条真的干活，见[本机运行器](#local_runner)；Pull Request 只有 `hosted` |
| CI result | `ubuntu-latest` | CI 的结论：Orchestrator 通过，并且有一条腿通过了应用测试。Release 等的是它 |

前两步本地用 `just lint` 执行，`just fmt` 按配置格式化。

- **编排器在 Linux 上测**：它的代码和测试不依赖 macOS（测试要 `git` 和 `python3`，运行环境自带），Linux 运行器几乎不用排队、CPU 也更多；macOS arm64 运行器紧张时一个任务要排 5 到 10 分钟，甚至排不到。产品只在 macOS 上运行，打包和发布仍在 macOS 上做，本地的 `just test` 也在 macOS 上。测试以普通用户运行（权限类测试要求文件权限生效），进程组用系统调用结束（`acp::signal_group`：Debian 的 `kill` 命令会把 `kill -TERM -<进程组>` 当成发给所有进程）。
- **应用只编译一次**：`swift test` 要用打开测试支持的设置编译，和普通的 `swift build` 不共用产物，先 `swift build` 再 `swift test` 会把整个应用编译两遍。
- **不缓存 SwiftPM 的 `.build`**：应用没有外部依赖，每次检出的源文件都比缓存新，缓存省不了编译，反而多花上传下载的时间。Rust 用 `Swatinem/rust-cache` 只缓存依赖。

<a id="release"></a>
## 发布

`.github/workflows/release.yml` 在推送 `v*` 标签时运行：

1. 安装 Rust。运行环境 `macos-15` 是 Apple 芯片，构建出的都是 arm64。不用 Rust 缓存：标签上存的缓存只有这个标签自己能读，下一次发布用不上，main 上的缓存是 debug 的，对 release 构建没用。
2. 不在这里跑测试：测试是 CI 的，标签指向的提交推到 main 时 CI 已经在跑，和这里的打包同时进行；发布前一步等这个提交在 main 上的 CI 给出结论（任务「CI result」，见[本机运行器](#local_runner)），没通过（失败、被取消）或找不到这个提交的 CI 时不发布。
3. 有 `MACOS_SIGNING_P12` secret 时，把证书导入临时钥匙串，设置 `JANUS_SIGN_IDENTITY` 和 `JANUS_SIGN_KEYCHAIN`，见[签名](#signing)；没有时用 ad-hoc 签名。
4. 执行 `frontend/scripts/package.sh --version <标签去掉 v>`（服务和应用互不依赖，同时编译），得到 `dist/Janus.app`、`dist/Janus-<版本>.dmg` 和[更新部分](#update_parts)。
5. 检查应用、服务和内置 Node.js 都是 arm64，校验签名；把两个更新部分重新拼装成应用再校验一次签名（更新器就是这样拼装的）；计算 SHA-256 写入 `SHA256SUMS.txt`（只写文件名，下载后在同一目录执行 `shasum -a 256 -c SHA256SUMS.txt` 即可核对）。
6. 上传为工作流产物；等这个提交的 CI 通过（见第 2 步）后创建同名的 GitHub Release（已存在时覆盖上传并刷新说明），附上 DMG、更新部分、`update.json` 和校验和。Release 说明由 `CHANGELOG.md` 中「`## <版本>`」一节（中英双语）加上中英文的首次打开步骤和系统权限提示组成；找不到这一节时工作流失败。首次打开的 Gatekeeper 放行与资源访问授权分别说明，升级后可在「设置 › 通用 › macOS 权限」处理后者。

发布一个版本（版本号按日期，见[版本号](#versioning)）：

```sh
# 先在 CHANGELOG.md 最上面写下这一版的「## 2026.11.01」一节
just e2e --packaged --real   # 打包版和真实 Agent 的端到端验证，见 docs/operations/20261031-testing.md#e2e
git push origin main          # 先推提交：CI 在 main 上测它，Release 发布前等这次 CI 通过
git tag v2026.11.01
git push origin v2026.11.01
```

每一版都要写 change log：小节标题是「`## <版本>`」，内容和标签里的版本一致；内容写用户能看到的变化，以及缺陷修复、性能优化和稳定性改进，内部的也写（用户看不到，但会影响他们的使用体验）；不涉及这些的纯内部重构不写。每节中英双语：先写中文条目，空一行，再写对应的英文条目；节内不用二级标题，工作流读到下一个「`## `」就停。工作流按标签取这一节作为 Release 说明，应用内更新的「更新内容」也读它，所以缺少这一节时不允许发布。

在 GitHub 的 Actions 页面手动运行 Release 工作流（填写版本号）只构建 DMG 并作为工作流产物上传，不创建 Release，用来试打包。`leg` 选 `local` 时只有本机运行器这条腿，不等托管机器，用来单独试[本机运行器](#local_runner)上的打包和签名（`gh workflow run release.yml -f version=0.0.0-dev -f leg=local`）。

<a id="versioning"></a>
## 版本号

- 发布版本按日期命名：标签 `v<年>.<月>.<日>`，例如 `v2026.11.01`，安装包为 `Janus-2026.11.01.dmg`。同一天再发布时加序号：`v2026.11.01.2`。
- 安装包的版本取自标签（去掉 `v`），写入 `Info.plist` 的 `CFBundleShortVersionString`；`CFBundleVersion` 是提交数，每次构建都递增。
- 本地打包不指定 `--version` 时，使用 `backend/Cargo.toml` 的版本；服务的 `janus --version` 也是它，与发布的日期版本无关。

<a id="update_parts"></a>
## 更新部分

应用内更新（见 [macOS 应用](../architecture/20261031-frontend.md#updates)）读取最新 Release 的这些附件，都由 `package.sh` 生成：

| 文件 | 内容 |
| --- | --- |
| `Janus-<版本>-app.tar.gz` | 不含 `Contents/Resources/node` 的 Janus.app，包括签名（签名覆盖 Node.js 的文件） |
| `Janus-<版本>-node.tar.gz` | `Contents/Resources/node` |
| `update.json` | `version`、`build`、`minimumSystemVersion` 和 `parts`：每个部分的 `name`（`app` / `node`）、`file`、`sha256`、`size`、`content` |

Node.js 部分的 `content` 是它所有文件内容和符号链接的哈希，同时写进应用的 `Contents/Resources/update-parts.json`。已安装应用的这个值与新版本相同时，更新器不下载 Node.js，直接复制本机的。打包时用 `COPYFILE_DISABLE=1` 和 `--no-mac-metadata`，压缩包里没有 AppleDouble 文件和扩展属性，解开后正好是签过名的文件。

<a id="signing"></a>
## 签名

`package.sh` 用 `JANUS_SIGN_IDENTITY`（钥匙串中证书的名字或 SHA-1，可用 `JANUS_SIGN_KEYCHAIN` 指定钥匙串）签名，没有设置时做 ad-hoc 签名。两者都不经过 Apple 公证。

- **固定的自签名证书**：`frontend/scripts/make-signing-cert.sh` 生成一张自签名的代码签名证书，默认保存在 `~/.config/janus/signing/`（不进仓库，也不在 Janus home：它是发版者的私钥，不是 Janus 的运行数据），并打印把它存成 Actions secrets `MACOS_SIGNING_P12`、`MACOS_SIGNING_PASSWORD` 的 `gh secret set` 命令。之后所有版本的指定要求都是 `identifier "io.github.duoyuli.janus" and certificate leaf = H"<证书 SHA-1>"`：macOS 把各个版本当作同一个应用，权限不会在更新后丢失；更新器也只接受满足这个要求的新版本。证书只生成一次，换证书后已安装的应用会拒绝更新，需要手动安装一次 DMG。
- **当前状态**：2026-11-18 生成了证书（SHA-1 `8D33F46FC46CC39DF847279A275A475666365A93`，私钥只在生成它的机器上，2026-11-21 起在 `~/.config/janus/signing/`，之前在 `~/.janus/signing/`，要手动移过去），并存为 Actions secrets；此后的发布都用它签名。同一版本（2026.11.18.5）把 bundle ID 从 `local.janus.app` 换成了 `io.github.duoyuli.janus`：签名身份本来就要变一次，一起换只让用户重新授权一次。应用的偏好设置（语言、外观、上次打开的设置页等）按 bundle ID 保存，换 id 后恢复默认，不做迁移；Janus home 中的数据不受影响。工作流把证书导入临时钥匙串并加进用户的钥匙串搜索列表（不在搜索列表中时，即使指定 `--keychain`，`codesign` 也找不到私钥），再从 `security find-identity` 输出中带编号的那一行取证书的 SHA-1：自签名证书不被系统信任，会列在「Matching identities」而不是「Valid identities」中，`codesign` 仍能用它签名。
- **ad-hoc**：没有配置 secrets 时使用。指定要求是二进制的 `cdhash`，每次构建都不同。更新器无法校验签名者，只依靠 HTTPS 和 SHA-256。
- **第一次安装**：自签名证书不被 Gatekeeper 信任，从浏览器下载的 DMG 第一次打开仍要在「隐私与安全性」中允许，见[部署方案](20261031-deployment.md#macos_app)。之后在应用内更新，不再需要。

以后有了 Developer ID 证书，把 `JANUS_SIGN_IDENTITY` 换成它、加上 `--options runtime`，并在工作流中增加 `notarytool` 公证和 `stapler` 步骤。设计取舍见 [ADR：应用内增量更新](../decisions/20261102-in_app_updates.md)。

<a id="local_runner"></a>
## 本机运行器

GitHub 托管的 macOS arm64 运行器容量不足时，任务排约 15 分钟后被 GitHub 取消。为了不卡住 CI 和发布，维护者的 Mac 上常驻一个自托管运行器，托管机器 5 分钟没接单就由它接手。决策和取舍见 [ADR](../decisions/20261124-local_runner_fallback.md)，风险见[安全边界：本机运行器](20261031-security.md#local_runner)。

**两条腿**：CI 的 `macos-app` 和 Release 的 `dmg` 都用矩阵从同一份步骤生成两个任务，`hosted`（`macos-15`）和 `local`（`[self-hosted, macOS, ARM64, janus-local]`）。检出之后的第一步是 `.github/scripts/claim-runner.sh`，它决定这条腿干不干活（输出 `work`），后面每一步都带 `if: steps.claim.outputs.work == 'true'`：

- `local` 立刻开始，每 10 秒看一次 `hosted`：5 分钟内它开始执行（或已经结束）就让出；否则接手，执行名为「Take over」的步骤。
- `hosted` 的机器到了以后，看 `local` 有没有成功的「Take over」步骤：有就让出，没有就干活。
- 托管机器恰好在本机接手的那一刻到达时，两条腿都会干活：测试跑两遍，安装包发布两次（后一次覆盖前一次），结果相同。
- CI 里只有推送到 `main` 的运行有 `local`；Pull Request 的运行只有 `hosted`，排不到就和原来一样失败。

**结论看「CI result」**：`hosted` 一直排不到时，GitHub 最后会取消它，整次运行显示失败，即使 `local` 已经通过。任务「CI result」是唯一的判定：Orchestrator 通过，并且某条腿的「Test」步骤成功；某条腿失败，或者所有腿都结束了却没有一条测过，就失败。Release 的「Wait for CI on this commit」读的是这个任务。Release 自己的运行在同样的情况下也会显示失败，安装包是否发出以 Release 页面和 `local` 那条腿为准。

**本机这条腿和托管的差别**：不装 Rust 工具链（`dtolnay/rust-toolchain` 会改掉维护者的默认工具链），用机器上已有的 Rust、Swift、Node.js 和 `gh`；签名步骤改过的钥匙串搜索列表在最后一步恢复。打出的安装包用的是本机的 SDK 和编译器，和 `macos-15` 上打出的不完全一样，最低系统版本和签名相同。

**安装**（已在维护者的机器上做过，换机器或重装时照做）：

```sh
# 外部贡献者的工作流一律先审批；先做这一步，再让运行器上线
gh api -X PUT repos/duoyuli/Janus/actions/permissions/fork-pr-contributor-approval -f approval_policy=all_external_contributors

# 装在不带用户名的路径下：工作目录会出现在公开的任务日志里
mkdir -m 700 /Users/Shared/janus-actions-runner && cd /Users/Shared/janus-actions-runner
V=2.338.2
curl -fsSL -o runner.tar.gz https://github.com/actions/runner/releases/download/v$V/actions-runner-osx-arm64-$V.tar.gz
shasum -a 256 runner.tar.gz   # 和 https://github.com/actions/runner/releases/tag/v$V 里 osx-arm64 的 SHA-256 核对
tar xzf runner.tar.gz && rm runner.tar.gz
./config.sh --url https://github.com/duoyuli/Janus --name janus-local-mac --labels janus-local --unattended --replace \
  --token "$(gh api -X POST repos/duoyuli/Janus/actions/runners/registration-token --jq .token)"
./svc.sh install                      # 登录后自动启动（LaunchAgent）
# 让任务跑在登录的图形会话里，见下文
plutil -replace SessionCreate -bool false ~/Library/LaunchAgents/actions.runner.duoyuli-Janus.janus-local-mac.plist
./svc.sh start
```

`svc.sh` 生成的 LaunchAgent 默认带 `SessionCreate`，任务跑在脱离图形登录会话的新会话里。应用测试里有靠合成输入事件验证的用例（`ChatScrollTests` 合成触控板滚动），在那样的会话里合成的手势不算人的操作，`aConversationKeepsThePlaceThePersonScrolledTo` 会失败，所以要关掉它。重装服务（`svc.sh uninstall` 再 `install`）后要重新改一次。机器要保持登录：没有图形会话时这些用例同样过不了。

运行器启动时的 `PATH` 在 `config.sh` 运行时记进 `.path`，要包含 `cargo`、`swift`、`node`、`gh` 和 `python3`。查看状态用 `./svc.sh status` 或 `gh api repos/duoyuli/Janus/actions/runners`。

**撤掉**：`./svc.sh stop && ./svc.sh uninstall`，再用 `./config.sh remove --token <移除令牌>` 注销（令牌来自 `gh api -X POST repos/duoyuli/Janus/actions/runners/remove-token`），最后删除目录。运行器不在线时，`local` 这条腿一直排队，`hosted` 照常工作；`hosted` 也排不到时，和没有本机运行器时一样失败。
