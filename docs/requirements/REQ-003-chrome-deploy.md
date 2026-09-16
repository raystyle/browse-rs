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

角色裁定（用户裁 2026-09-16，架构决策见 [ADR-0007](../adr/ADR-0007-embedded-chromium-manager.md)）：**browse 内嵌 Chromium 版本管理器，不依赖外部安装**；各版本 Chromium 在 browse 自有应用数据目录下版本化管理；**omc 负责 clean-chrome 发布包的资源分发**（R2 镜像 + 版本段路由 + `.sha256` 边车锚，同 ark/hst 分发链）；**ark 不管 clean-chrome 安装**（明确排除，非 ark catalog 面）。clean-chrome 工位转话确认（commit c002325 在册）：分发走 omc 体系、安装执行面非 ark，均与本裁定同向；部署形态参照本仓应用数据目录版本段子目录范式（152 保留回退位，clean-chrome REQ-012 的 155 双版本过渡在册）。

## Criteria

- [x] 部署·本地导入面：`browse chrome install <版本> <部署目录>`（方言 `chromeInstall({fromDir, version?})`）整目录复制到 `<state>/chromium/<version>/`，校验 chrome 二进制在位、manifest 登记、自动 pin（Windows 真机冒烟过：500 文件/683MB）
- [ ] 部署·R2 下载腿：镜像域版本段 `<tool>/<version>/<asset>` + `.sha256` 边车锚校验后原子落位（**待 omc 端点定标**，端点载体与清单形态在册待协调）
- [x] 升级与 pin：`chromeUse(version)` 切换托管位，旧版保留可回退（版本段范式对齐 clean-chrome REQ-012 的 152 回退位与 155 双版本过渡）；升级走新版本号 install + use，不迁移不动用户数据
- [x] 维护：`chromeDoctor()`（逐版本在位、文件数基线、pin 健康；盘面漂移时托管位自动退发现序）
- [x] 自有用户数据：user-data 与 engine-profile 绑 `<state>/` 跨版本持久（ADR-0003 附着优先不碰用户浏览器，行为未动）
- [x] 发现序衔接：`--chrome` 显式 -> `BROWSE_CHROME` -> 托管 pin -> cwd/exe 祖先 `chromium-*` -> 常规路径（engine `resolve_chrome` 落地）
- [x] 版本登记：`<state>/chromium/manifest.json`（已装版本 + 来源 + 文件/字节基线 + pin）
- [x] 命令面：8 条登记 `COMMANDS` 单一真相源（Cli 4 + 全局 4），schema/llms/skill 三面派生，surface_contract 锁漂移
- [ ] 定标项（候 omc 协调或派单裁定）：R2 镜像 tool 段命名与版本清单端点、`BROWSE_CHROME_MIRROR` 是否默认化、缺版本时自动安装还是 CTA（现裁 CTA）、跨平台资产清单（今日仅 Windows 部署形态实证）、GitHub 回退腿取舍

进度注：本地导入面全链已实现并四面门禁绿；R2 下载腿完成前保持 draft（判据未全过，trace 不回填）。按 semver 判据属能力新增，落 0.1.0 封版后的 0.2.0 批。
