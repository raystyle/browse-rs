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

角色裁定（用户裁 2026-09-16，架构决策见 [ADR-0007](../adr/ADR-0007-embedded-chromium-manager.md)）：**browse 内嵌 Chromium 版本管理器，不依赖外部安装**；各版本 Chromium 在 browse 自有应用数据目录下版本化管理；**omc 负责 clean-chrome 发布包的资源分发**，承载定令（用户令 2026-09-16，clean-chrome 工位转达）：**ohmygh.COM 域下专门子域名，omc 承建**，路由同 R2 镜像 + 版本段 + `.sha256` 边车锚（ark/hst 分发链同构）；**ark 不管 clean-chrome 安装**（明确排除，非 ark catalog 面）。本仓**勿自建分发腿**。子域名定标回执已到（omc 总台，2026-09-16）：**chrome.ohmygh.com**（与 env/pkgs/registry 平级），R2 桶已建，版本段路由 `<version>/<asset>` + `.sha256` 边车（与 env 域同构，无 manifest，边车即锚）；DNS CNAME 至 public.r2.dev 传播中，**总台热验回报后再接实测**。clean-chrome 工位转话确认（commit c002325 在册）：分发走 omc 体系、安装执行面非 ark，均与本裁定同向；部署形态参照本仓应用数据目录版本段子目录范式（152 保留回退位，clean-chrome REQ-012 的 155 双版本过渡在册）。

## Criteria

- [x] 部署·本地导入面：`browse chrome install <版本> <部署目录>`（方言 `chromeInstall({fromDir, version?})`）整目录复制到 `<state>/chromium/<version>/`，校验 chrome 二进制在位、manifest 登记、自动 pin（Windows 真机冒烟过：500 文件/683MB）
- [x] 部署·R2 下载腿：`https://chrome.ohmygh.com/<version>/<asset>` 下载 + 同名 `.sha256` 边车锚校验后原子落位（总台热验回执 2026-09-17：端点验讫，HTTP/HTTPS/H2 通、HKG 边缘在位、TLS 链净）。实现：CLI `browse chrome install <版本>`（部署目录缺省即镜像下载）、方言 `chromeInstall({version})`；实测：mock 镜像三态全绿（happy 落位登记 pin 零残件、锚不匹配错包即弃、404 带 CTA）加真端点负测（404 时错误带端点与 `BROWSE_CHROME_ASSET` 覆写指引，exit 1）；happy-path 真资产实测随 clean-chrome 首版资产落桶
- [x] 升级与 pin：`chromeUse(version)` 切换托管位，旧版保留可回退（版本段范式对齐 clean-chrome REQ-012 的 152 回退位与 155 双版本过渡）；升级走新版本号 install + use，不迁移不动用户数据
- [x] 维护：`chromeDoctor()`（逐版本在位、文件数基线、pin 健康；盘面漂移时托管位自动退发现序）
- [x] 自有用户数据：user-data 与 engine-profile 绑 `<state>/` 跨版本持久（ADR-0003 附着优先不碰用户浏览器，行为未动）
- [x] 发现序衔接：`--chrome` 显式 -> `BROWSE_CHROME` -> 托管 pin -> cwd/exe 祖先 `chromium-*` -> 常规路径（engine `resolve_chrome` 落地）
- [x] 版本登记：`<state>/chromium/manifest.json`（已装版本 + 来源 + 文件/字节基线 + pin）
- [x] 命令面：8 条登记 `COMMANDS` 单一真相源（Cli 4 + 全局 4；安装面扩两源后签名同册），schema/llms 两面派生（skill 物种 2026-09-16 退役，--llms 即说明书），surface_contract 锁漂移
- [ ] 定标项（余量）：可用版本发现来源（子域无 manifest 面；候选：显式传版本号、clean-chrome 仓 release 元数据；v1 已落显式传版本号，镜像下载腿无 version 即 CTA）、资产命名形态（暂定约定 `chromium-<version>.zip`，`BROWSE_CHROME_ASSET` 全名覆写；随总台首版资产确认改正式）、跨平台资产清单（Windows 部署形态实证；Linux 运行时集实证可跑：lan-linux 与 wsl 无头全链 2026-09-17，资产打包形候 155 窗；mac 待）；`BROWSE_CHROME_MIRROR` 载体已落（默认 `https://chrome.ohmygh.com`，覆写走测试与自建镜像）；缺版本 CTA 已裁（不自动装）；分发腿不自建（用户令裁定，GitHub 直连腿同废）

进度注：本地导入面与 R2 下载腿均已实现（下载腿真资产 happy-path 随首版资产补测）；定标余量三项未闭（版本发现来源正式定标、资产命名正式化、跨平台资产清单）前保持 draft（trace 不回填）。按 semver 判据属能力新增，落 0.2.0 批。
