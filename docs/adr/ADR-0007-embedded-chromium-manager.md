---
id: ADR-0007
status: accepted
date: 2026-09-16
deciders: 用户裁定（架构定调周知）+ 维护者
---

# ADR-0007：内嵌 Chromium 版本管理器托管 clean-chrome

关联：[ADR-0001](ADR-0001-daemon-persistent.md)（常驻 daemon）、[ADR-0003](ADR-0003-attach-first-spawn-fallback.md)（引擎策略）、[ADR-0006](ADR-0006-multi-instance-name.md)（应用数据目录）；clean-chrome 工位同向确认（转话 commit c002325），其 REQ-012 的 155 双版本过渡与本仓版本段子目录范式互为参照。

## Context

browse 的引擎是 clean-chrome（Chromium 品牌），当前发现序依赖仓库旁的 `chromium-*/` 部署产物（gitignore、随仓库走）：装进 PATH 的裸二进制在无仓机器上找不到引擎，附着探测失败后 spawn 必然报「找不到 chrome」。fleet 分发体系同日成局（omc 管资源分发、ark 管落地执行），需要裁定 clean-chrome 在其中的归属与 browse 的集成形态（用户架构定调 2026-09-16）。

## Decision

browse 内嵌 Chromium 版本管理器，不依赖外部安装：

1. **自有应用数据目录内版本化管理**：各版本 Chromium 落 `<state>/chromium/<version>/`（安装、部署、升级一体）；安装部署升级机制复制 clean-chrome 既有各版本形态，不自创。
2. **资源分发归 omc**：clean-chrome 发布包走 omc 分发面，承载 **chrome.ohmygh.com**（ohmygh.COM 专门子域，omc 承建，定标回执 2026-09-16），路由 `<version>/<asset>` + `.sha256` 边车即锚（无 manifest，与 env 域同构）；browse 只做下载校验与落位，**不自建分发腿**（含 GitHub 直连腿）；DNS 热验回报后接实测。
3. **ark 明确排除**：clean-chrome 安装不属 ark catalog 面，安装执行面在 browse 自身。
4. 引擎策略不变：附着优先（ADR-0003）、用户数据与版本目录分离（engine-profile 跨版本持久，升级零迁移）。

## Consequences

- 好：装 browse 即得自管理引擎，无仓机器可用；版本可 pin 可回退；升级不动用户数据；与 fleet 分工清晰（omc 分发、browse 安装、ark 不涉）。
- 坏：安装引入网络依赖与子域名可用性风险（边车锚校验兜底；omc 端点未定标前 R2 腿不可用，本地导入面先行）；多版本并存占盘（需 remove 面或手动清理）；跨平台资产清单依赖 clean-chrome 侧定标。

## Alternatives

- 捆绑发行（把 Chromium 打进 browse 包体）：包体巨大，引擎升级被迫随发版走，否决。
- ark 统一安装（fleet 常规工具链路）：用户明确排除，clean-chrome 非 ark catalog 面。
- 仅 GitHub 直连下载：绕开 omc 分发链与边车锚标准，与 fleet 定调相悖，否决；用户令 2026-09-16 加钉「勿自建分发腿」，回退腿候选资格同废（端点未定标期由本地导入面顶位）。
