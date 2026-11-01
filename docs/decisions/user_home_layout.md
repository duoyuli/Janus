# ADR：用户数据、依赖和日志都放在 Janus home

- 状态：已采纳（accepted），记录于 2026-10-30。
- 相关文档：[部署方案](../operations/deployment.md#layout)、[数据模型与存储](../architecture/data_model.md#locations)、[配置](../interfaces/configuration.md#settings)、[ADR：每个 Agent 使用独立的配置目录](isolated_agent_homes.md)。

## 背景

早期版本把数据拆在两个地方：数据库、Agent 配置和凭据在 `~/.janus`，而 ACP 适配器装在仓库的 `third_party/`，产物、日志和 Skill 快照写在仓库的 `.janus/`。用户指出这样不对：仓库应该只是代码；用户级数据应该放在用户目录下；运行时需要的全局依赖（例如 ACP 的包）应该下载到全局目录；用户的配置和日志也应该放在全局目录。

拆开存放还有实际问题：数据库里保存的是产物的绝对路径，移动或删除仓库，旧 Run 的产物就失效；每个仓库副本都要重新安装约 850M 的适配器；Janus 也无法脱离仓库运行。

## 决策

1. 仓库只放代码，运行时不向仓库写任何文件。
2. 用户级数据（数据库、Agent 配置与凭据、Skill 库、产物、Skill 快照）、全局依赖（`adapters/`）、配置（`config.toml`）和日志（`logs/`）都在 Janus home（`JANUS_HOME`，默认 `~/.janus`）。
3. 配置项按「环境变量 → `config.toml` → 默认值」读取。
4. 安装到 `~/.janus/bin/janus` 的服务和安装包中的 `Janus.app` 都不依赖仓库。
5. `/api/file` 只能读取 `artifacts/`，因为 Janus home 里还有数据库和凭据。
6. 现有数据一次性搬到新位置，数据库中的旧路径一并改写；代码中不保留旧布局的兼容逻辑。

## 取舍

- **与按仓库存放相比**：数据和仓库互不影响，多个仓库副本共用同一套适配器和数据；代价是不同的检出不能各自隔离数据，需要隔离时设置不同的 `JANUS_HOME`。
- **Janus home 同时有数据和凭据**：所有经由 HTTP 读取文件的接口都必须限定在明确的子目录内。

## 后果

- 卸载程序（`just uninstall`）不影响数据；备份只需复制 `~/.janus`。
- 新增任何会写盘的功能，都要先确定它属于 Janus home 的哪个子目录，并同步更新[部署方案](../operations/deployment.md#layout)。
