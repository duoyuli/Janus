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
3. 有 `MACOS_SIGNING_P12` secret 时，把证书导入临时钥匙串，设置 `JANUS_SIGN_IDENTITY` 和 `JANUS_SIGN_KEYCHAIN`，见[签名](#signing)；没有时用 ad-hoc 签名。
4. 执行 `frontend/scripts/package.sh --version <标签去掉 v>`，得到 `dist/Janus.app`、`dist/Janus-<版本>.dmg` 和[更新部分](#update_parts)。
5. 检查应用、服务和内置 Node.js 都是 arm64，校验签名；把两个更新部分重新拼装成应用再校验一次签名（更新器就是这样拼装的）；计算 SHA-256 写入 `SHA256SUMS.txt`（只写文件名，下载后在同一目录执行 `shasum -a 256 -c SHA256SUMS.txt` 即可核对）。
6. 上传为工作流产物，并创建同名的 GitHub Release（已存在时覆盖上传），附上 DMG、更新部分、`update.json` 和校验和，发布说明包含首次打开的步骤。

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

<a id="update_parts"></a>
## 更新部分

应用内更新（见 [macOS 应用](../architecture/frontend.md#updates)）读取最新 Release 的这些附件，都由 `package.sh` 生成：

| 文件 | 内容 |
| --- | --- |
| `Janus-<版本>-app.tar.gz` | 不含 `Contents/Resources/node` 的 Janus.app，包括签名（签名覆盖 Node.js 的文件） |
| `Janus-<版本>-node.tar.gz` | `Contents/Resources/node` |
| `update.json` | `version`、`build`、`minimumSystemVersion` 和 `parts`：每个部分的 `name`（`app` / `node`）、`file`、`sha256`、`size`、`content` |

Node.js 部分的 `content` 是它所有文件内容和符号链接的哈希，同时写进应用的 `Contents/Resources/update-parts.json`。已安装应用的这个值与新版本相同时，更新器不下载 Node.js，直接复制本机的。打包时用 `COPYFILE_DISABLE=1` 和 `--no-mac-metadata`，压缩包里没有 AppleDouble 文件和扩展属性，解开后正好是签过名的文件。

<a id="signing"></a>
## 签名

`package.sh` 用 `JANUS_SIGN_IDENTITY`（钥匙串中证书的名字或 SHA-1，可用 `JANUS_SIGN_KEYCHAIN` 指定钥匙串）签名，没有设置时做 ad-hoc 签名。两者都不经过 Apple 公证。

- **固定的自签名证书**：`frontend/scripts/make-signing-cert.sh` 生成一张自签名的代码签名证书，保存在 `~/.janus/signing/`（不进仓库），并打印把它存成 Actions secrets `MACOS_SIGNING_P12`、`MACOS_SIGNING_PASSWORD` 的 `gh secret set` 命令。之后所有版本的指定要求都是 `identifier "local.janus.app" and certificate leaf = H"<证书 SHA-1>"`：macOS 把各个版本当作同一个应用，权限不会在更新后丢失；更新器也只接受满足这个要求的新版本。证书只生成一次，换证书后已安装的应用会拒绝更新，需要手动安装一次 DMG。
- **ad-hoc**：指定要求是二进制的 `cdhash`，每次构建都不同。更新器无法校验签名者，只依靠 HTTPS 和 SHA-256。
- **第一次安装**：自签名证书不被 Gatekeeper 信任，从浏览器下载的 DMG 第一次打开仍要在「隐私与安全性」中允许，见[部署方案](deployment.md#macos_app)。之后在应用内更新，不再需要。

以后有了 Developer ID 证书，把 `JANUS_SIGN_IDENTITY` 换成它、加上 `--options runtime`，并在工作流中增加 `notarytool` 公证和 `stapler` 步骤。设计取舍见 [ADR：应用内增量更新](../decisions/in_app_updates.md)。
