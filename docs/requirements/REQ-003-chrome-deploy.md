---
id: REQ-003
title: browse 托管 clean-chrome：部署升级维护与自有用户数据
status: draft
priority: must
trace: null
---

# REQ-003：browse 托管 clean-chrome：部署升级维护与自有用户数据

## Scenario

agent 或用户在任意系统只装 browse 一个软件包：browse 自带独立用户数据目录（绝不碰系统浏览器与用户默认 profile），并负责该系统上 clean-chrome 的部署、升级与维护（用户令 2026-09-16）。

## Criteria

- [ ] 部署：一条命令在 Windows/Linux/macOS 部署 clean-chrome 到本仓状态目录（下载走 sha256 边车锚校验，对齐 flow-release 第八节分发标准）
- [ ] 升级：版本查询、升级与版本 pin；升级不迁移不动用户数据目录
- [ ] 维护：doctor 面（部署完整性、当前版本、profile 独占检查）
- [ ] 自有用户数据：clean-chrome 的 user-data 与引擎 profile 绑定落 `<state>/`，现行为照旧（ADR-0003 附着优先不碰用户浏览器）
- [ ] 发现序衔接：托管部署位并入现有发现序（BROWSE_CHROME 显式 -> 托管位 -> cwd/exe 祖先 `chromium-*` -> 常规路径），优先级在定标批裁定
- [ ] 分发源：镜像域版本段路由优先、GitHub 回退（三通道与边车锚口径同 flow-release 第八节）
- [ ] 命令面：新命令登记 `COMMANDS` 单一真相源，schema/llms/skill 三面派生，surface_contract 锁漂移
- [ ] 定标项（开工前问询或派单裁定）：镜像域名与 tool 段命名、版本 pin 载体形态、跨平台资产清单、部署目录命名

实现批须先立 ADR-0007（分发源与部署位是不可逆选择）；按 semver 判据属能力新增，落 0.1.0 封版后的 0.2.0 批。
