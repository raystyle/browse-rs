---
id: REQ-005
title: browse update 自更新面（家族统一标准件）
status: implemented
priority: should
trace: 单测五件（self_update tests：资产名形、semver 判新、解包唯一命中、自替换回滚、双通道三态）+ 实弹（upToDate 幂等真 API 链、ark 落痕拦 CTA、评审侧 0.6.0→0.6.1 双通道全链实测）
---

# REQ-005：browse update 自更新面

## Scenario

browse 需要运行时自升级能力，按家族（ark/hst/reader/omc）统一自升级标准对齐落地（hst 与 ark 已有自更新面，本仓是按 build-release 公共契约第六节对齐的最新一仓；用户令 2026-09-18，经飞轮转呈其余仓）。

## Criteria

- 命令形：顶层子命令 `browse update`（build-release 公共契约第六节「可执行 CLI 且自身带 update 子命令」；与 `browse chrome update` 引擎域不撞）
- 双通道：资产下载自家镜像 stable 滚动段优先（`browse.ohmygh.com/browse/stable/`），任一步失败整对回落 GitHub Releases download；资产与边车恒同源；`.sha256` 边车锚校验与发布器同 digest 判据，哈希不符硬拒不回落
- 判新：GitHub Releases latest API（tag 去 v）；semver 只升不降，本地领先报 localNewer 不动
- 自替换：暂存落 exe 同目录（防跨文件系统 rename EXDEV）加 pid 后缀（防并发互踩）加更新锁；`--version` 自证带五次重试（杀软瞬时锁面）；证败回滚并复核终态，回滚受阻报自救路径
- ark 单通道让位：exe 同目录 `ark-managed` 落痕或用户面 bin 符号链接入口（ark 布局）即拦，CTA 走 ark；落痕生产者契约派 ark 侧（飞轮转呈）
- 环境覆写：`BROWSE_RELEASE_MIRROR`；GitHub token（`GH_TOKEN`/`GITHUB_TOKEN`）在位自动附 Bearer 提限流
- 平台边界：aarch64/musl Linux 无发布资产，错误带源码安装 CTA

## 裁定

- 判新走 GitHub tag 而非镜像 stable 段（段内资产名带版本无法反查），与 chrome 引擎的 latest 指针口径分家并存
- 版本判定：能力新增取 minor（0.7.0 批）
