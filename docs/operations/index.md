# 运维文档索引

> 上级索引：[工程文档](../index.md)

## 范围

本分类记录怎样部署、运行、重启、排查和测试 Janus，以及它的安全边界。接口参数和环境变量的定义归入[接口](../interfaces/index.md)。

## 文档列表

- [部署方案](20261031-deployment.md)：目录规范（Janus home 的分类、新增文件放在哪里）、macOS 应用的安装与构建、前置条件、安装、以服务方式启动、日常操作、升级、备份与卸载，以及 `justfile` 中的命令。阅读时机：安装或升级 Janus，修改 `justfile` 或 launchd 配置，或新增要写盘的功能需要确定放在哪个目录。
- [CI 与发布](20261101-ci_release.md)：GitHub Actions 的测试工作流、推送版本标签时构建并发布 DMG、版本号、签名，以及 GitHub 的 macOS 运行器排不到时接手的本机运行器。阅读时机：修改 `.github/workflows/`、`frontend/scripts/package.sh`，或发布新版本。
- [运行手册](20261031-runbook.md)：第一次运行、重启与恢复、按现象排查、数据清理。阅读时机：日常使用中重启服务，或排查 Agent 检查失败、适配器安装失败、Run 卡住、界面不更新。
- [测试](20261031-testing.md)：测试分布、每个需求完成后的端到端验证（`just e2e`）、产品端到端测试的场景、脚本化 ACP Agent、重拍界面截图（`just screenshots`）、`flow_tests` 夹具和编写测试的约定。阅读时机：运行或编写测试，完成一个需求准备合并，修改 `e2e/`、`backend/testdata/fake_acp.py` 或应用的快照命令，或测试随机失败。
- [安全边界](20261031-security.md)：Agent 运行环境隔离、复制本机配置的范围、凭据保存、工作目录访问、网络暴露、适配器安装、资源库、应用更新、macOS 权限引导和模型价格的来源。阅读时机：修改 `spec_for`、`sandbox_env`、配置复制或凭据保存，改变监听地址、修改系统权限申请与检查，或评估 Janus 能否用于某个项目。
