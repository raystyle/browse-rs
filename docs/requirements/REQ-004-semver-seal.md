---
id: REQ-004
title: semver 判据与封版流（0.1.0 首封）
status: implemented
priority: must
trace: tag v0.1.0（git 自取）+ 三路门禁回执见 diary 2026-09-16 封版节
---

# REQ-004：semver 判据与封版流（0.1.0 首封）

## Scenario

仓走到可发布态，需要把版本判定判据在册（AGENTS Must 要求判据落在封版 REQ）、执行首封，并钉死封版后批次的版本走向。口径对齐 flow-release 第七节（多仓版本标准）。

## Criteria

- semver 判据（在册，封版后长期有效）：文档/修复批取 patch；能力新增或行为变化取 minor；契约破裂或形态重构取 major
- 0.1.0 判定：零 tag 零 CHANGELOG 起步，现存全部功能（REQ-001 文档体系、REQ-002 `--llms` 通道、REQ-003 本地导入面、daemon/方言/引擎主体、CI 三岗）皆 0.1.0 主体，无前置版可破坏，故首封即 0.1.0 不取 1.0.0
- 封版后走向：REQ-003 R2 下载腿属能力新增，落地批取 0.2.0；其余批按判据对号
- [x] 三路门禁首封全绿（wsl 全件含 release 构建；lan-ubuntu/lan-mac 实机 rsync 同树 clippy 加 12 组 test；CI main 双岗；lan-win=win-gnu 交叉岗、lan-linux=备用端点，均在册口径）
- [x] CHANGELOG 立卷定版 0.1.0（版本级里程碑制）
- [x] tag v0.1.0 推远端
- 分发腿不预建（用户裁定在册，2026-09-16 时点）：CLI 资产分发归 omc/seed 通道，首发资产窗口随总台调度
- 追正（总台核准 2026-09-17 批一/批二）：发布流水已在仓内落地（seed-only workflow 加 tools/release.pwsh 本地发布面，build-release 标准三段式），上句「仓内不建 release 流水线」按新核准形态废止；总台只剩 catalog pin 滚
