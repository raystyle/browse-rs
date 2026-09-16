---
id: REQ-003
title: browse 托管 clean-chrome：部署升级维护与自有用户数据
status: draft
priority: must
trace: null
---

# REQ-003：browse 托管 clean-chrome：部署升级维护与自有用户数据

## Scenario

agent 或用户在任意系统只装 browse 一个软件包：browse 自带独立应用数据目录（绝不碰系统浏览器与用户默认 profile），并负责该系统上 clean-chrome 的部署、升级与维护（用户令 2026-09-16）。

角色裁定（用户裁 2026-09-16，架构决策见 [ADR-0007](../adr/ADR-0007-embedded-chromium-manager.md)）：**browse 内嵌 Chromium 版本管理器，不依赖外部安装**；各版本 Chromium 在 browse 自有应用数据目录下版本化管理；**omc 负责 clean-chrome 发布包的资源分发**（R2 镜像 + 版本段路由 + `.sha256` 边车锚，同 ark/hst 分发链）；**ark 不管 clean-chrome 安装**（明确排除，非 ark catalog 面）。

## Criteria

- [ ] 部署：一条命令在 Windows/Linux/macOS 部署 clean-chrome 到 `<state>/chromium/<version>/`（下载走 R2 镜像版本段 + 边车锚校验，原子落位）；机制复制 clean-chrome 既有各版本安装部署形态
- [ ] 升级：版本发现、安装、pin 切换与升级（旧版本保留可回退）；升级不迁移不动用户数据
- [ ] 维护：doctor 面（部署完整性、已装版本与 pin、profile 独占检查）
- [ ] 自有用户数据：clean-chrome 的 user-data 与引擎 profile 绑定落 `<state>/`（engine-profile 跨版本持久），现行为照旧（ADR-0003 附着优先不碰用户浏览器）
- [ ] 发现序衔接：托管 pin 版本并入现有发现序（BROWSE_CHROME 显式 -> 托管 pin -> cwd/exe 祖先 `chromium-*` -> 常规路径），优先级在定标批裁定
- [ ] 版本登记：`<state>/chromium/manifest.json` 记已装版本、pin 指向与 digest
- [ ] 命令面：新命令登记 `COMMANDS` 单一真相源，schema/llms/skill 三面派生，surface_contract 锁漂移
- [ ] 定标项（开工前问询或派单裁定）：镜像 tool 段命名与版本清单端点（与 omc 协调）、缺版本时自动安装还是 CTA、跨平台资产清单、GitHub 回退腿取舍

按 semver 判据属能力新增，落 0.1.0 封版后的 0.2.0 批。
