# 运维文档索引

> 上级索引：[工程文档](../index.md)

## 范围

本分类记录怎样部署、运行、重启、排查和测试 Janus，以及它的安全边界。接口参数和环境变量的定义归入[接口](../interfaces/index.md)。

## 文档列表

- [部署方案](deployment.md)：目录规范、macOS 应用的安装与构建、前置条件、安装、以服务方式启动、日常操作、升级、备份与卸载，以及 `justfile` 中的命令。阅读时机：安装或升级 Janus，修改 `justfile` 或 launchd 配置，或新增要写盘的功能需要确定放在哪个目录。
- [CI 与发布](ci_release.md)：GitHub Actions 的测试工作流、推送版本标签时构建并发布 DMG、版本号和签名。阅读时机：修改 `.github/workflows/`、`frontend/scripts/package.sh`，或发布新版本。
- [运行手册](runbook.md)：第一次运行、重启与恢复、按现象排查、数据清理。阅读时机：日常使用中重启服务，或排查 Agent 检查失败、适配器安装失败、Run 卡住、界面不更新。
- [测试](testing.md)：测试分布、脚本化 ACP Agent、`flow_tests` 夹具和编写测试的约定。阅读时机：运行或编写测试，修改 `backend/testdata/fake_acp.py`，或测试随机失败。
- [安全边界](security.md)：Agent 运行环境隔离、复制本机配置的范围、凭据保存、工作目录访问、网络暴露和 适配器安装、Skill 导入。阅读时机：修改 `spec_for`、`sandbox_env`、配置复制或凭据保存，改变监听地址，或评估 Janus 能否用于某个项目。
