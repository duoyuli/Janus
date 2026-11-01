# CI 与发布

本文说明 GitHub Actions 中的两条工作流：每次推送运行的测试，以及推送版本标签时构建并发布 macOS 安装包。打包脚本本身见[部署方案](deployment.md#macos_app)。

<a id="ci"></a>
## 持续集成

`.github/workflows/ci.yml` 在推送到 `main` 和每个 Pull Request 时运行，同一分支的新推送会取消还在跑的旧任务：

| 任务 | 运行环境 | 内容 |
| --- | --- | --- |
| Orchestrator | `macos-15` | `cargo test --locked`（Rust 编排器的全部测试，含 `flow_tests`） |
| macOS app | `macos-15` | `swift build`、`swift test`（应用的构建和 Swift Testing 测试） |

两项都在 macOS 上运行：编排器的测试需要 `git` 和 `python3`，应用的测试需要 Xcode，运行环境都自带。

<a id="release"></a>
## 发布

`.github/workflows/release.yml` 在推送 `v*` 标签时运行：

1. 安装 Rust。运行环境 `macos-15` 是 Apple 芯片，构建出的都是 arm64。
2. 运行编排器和应用的测试，任一失败就不发布。
3. 执行 `macos/scripts/package.sh --version <标签去掉 v>`，得到 `dist/Janus.app` 和 `dist/Janus-<版本>.dmg`。
4. 检查应用、服务和内置 Node.js 都是 arm64，校验签名，计算 SHA-256 写入 `SHA256SUMS.txt`（只写文件名，下载后在同一目录执行 `shasum -a 256 -c SHA256SUMS.txt` 即可核对）。
5. 上传为工作流产物，并创建同名的 GitHub Release（已存在时覆盖上传），附上 DMG 和校验和，发布说明包含首次打开的步骤。

发布一个版本（版本号按日期，见[版本号](#versioning)）：

```sh
git tag v2026.11.01
git push origin v2026.11.01
```

在 GitHub 的 Actions 页面手动运行 Release 工作流（填写版本号）只构建 DMG 并作为工作流产物上传，不创建 Release，用来试打包。

<a id="versioning"></a>
## 版本号

- 发布版本按日期命名：标签 `v<年>.<月>.<日>`，例如 `v2026.11.01`，安装包为 `Janus-2026.11.01.dmg`。同一天再发布时加序号：`v2026.11.01.2`。
- 安装包的版本取自标签（去掉 `v`），写入 `Info.plist` 的 `CFBundleShortVersionString`；`CFBundleVersion` 是提交数，每次构建都递增。
- 本地打包不指定 `--version` 时，使用 `backend/Cargo.toml` 的版本；服务的 `janus --version` 也是它，与发布的日期版本无关。

<a id="signing"></a>
## 签名

安装包只做 ad-hoc 签名，没有 Apple 开发者证书，也不经过公证。从网上下载的应用第一次打开会被 Gatekeeper 拦下，用户需要手动允许一次，步骤见[部署方案](deployment.md#macos_app)。以后有了 Developer ID 证书，在 `package.sh` 中把 `codesign --sign -` 换成证书身份、加上 `--options runtime`，并在工作流中增加 `notarytool` 公证和 `stapler` 步骤即可，证书和密码放在仓库的 Actions secrets 中。
