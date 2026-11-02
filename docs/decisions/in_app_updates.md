# ADR：应用内增量更新，用固定的自签名证书签名

- 状态：accepted
- 日期：2026-11-02
- 补充：[用原生 macOS 应用做展示层](native_macos_app.md)中的签名方式

## 背景

此前每个版本都是浏览器下载的 DMG，只做 ad-hoc 签名。浏览器给下载的文件加上 `com.apple.quarantine`，所以每次升级都要在「系统设置 → 隐私与安全性」中点「仍要打开」。ad-hoc 签名的指定要求（designated requirement）是二进制自身的哈希 `cdhash`，每次构建都不同，macOS 把每个版本当作新应用，已经给过的权限也要重新授予。应用包 146 MB，其中 133 MB 是很少变化的 Node.js，每次都要整包下载。

## 决策

- 应用自己从 GitHub Release 下载更新：URLSession 下载的文件没有隔离标记，Gatekeeper 不会拦下。只有第一次从 DMG 安装时需要手动允许。
- 发布拆成两个部分：不含 Node.js 的应用（约 5 MB）和 Node.js。`update.json` 记录每个部分的 SHA-256 和内容哈希，Node.js 没有变化时沿用本机的副本，只下载应用部分。
- 发布用一张固定的自签名代码签名证书签名（`frontend/scripts/make-signing-cert.sh` 生成，存为 Actions secrets）。指定要求变成 `identifier "local.janus.app" and certificate leaf = H"…"`，各个版本都是同一个应用，权限不再丢失。更新器要求新版本满足当前版本的指定要求，别人签名的包会被拒绝。
- 更新在退出后替换：脚本等应用退出，把旧版本移到 `~/.janus/updates/previous/`，放入新版本并重新打开。

## 取舍

- **与 Apple Developer ID 和公证相比**：没有证书费用。代价是第一次从 DMG 安装仍要手动允许一次；之后的更新不需要。将来有 Developer ID 时，把 `JANUS_SIGN_IDENTITY` 换成它、加上公证步骤即可，更新器不用改。但换证书会改变指定要求，用旧证书签名的已安装版本会拒绝新证书的更新，届时需要手动安装一次。
- **与 Sparkle 相比**：Sparkle 需要 Xcode 工程或手工嵌入框架、单独的 EdDSA 密钥和 appcast 生成步骤。Janus 的包结构固定，按部分更新和签名者校验一百多行代码就能完成，发布仍是现有的 GitHub Release。
- **与二进制差分（bsdiff）相比**：差分需要保留每个旧版本的包并生成多份补丁；按部分更新已经把常见的升级从约 50 MB 降到约 5 MB。
- **信任**：未设置签名证书时（ad-hoc 构建），更新器只能依靠 HTTPS 和 `update.json` 中的 SHA-256；设置之后才有签名者校验。持有 `.p12` 的人可以签出应用会接受的更新，它和 Actions secrets 一样需要保管好。

## 后果

- 发布多出 `Janus-<版本>-app.tar.gz`、`Janus-<版本>-node.tar.gz` 和 `update.json`，工作流在发布前把两部分重新拼装并校验签名。
- 应用读取 Info.plist 的 `JanusUpdateRepo` 决定更新来源；从源码运行时不检查更新。
- 从 ad-hoc 版本升级到第一个用证书签名的版本时，因为旧版本没有可要求的签名者，只校验完整性；从那以后都校验签名者。

## 相关文档

- [CI 与发布](../operations/ci_release.md#signing)
- [macOS 应用：应用内更新](../architecture/frontend.md#updates)
- [部署方案：升级](../operations/deployment.md#upgrade)
