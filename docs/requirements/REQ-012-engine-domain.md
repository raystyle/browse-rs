---
id: REQ-012
title: 引擎层 Browse.* 扩展域批（issue #27，与 clean-chrome REQ-015 联动）
status: in_progress
priority: should
trace: null
---

# REQ-012：引擎层 Browse.* 扩展域批（issue #27）

## Scenario

引擎内计算、CLI 只收结论：语义快照直出、变更推送、响应订阅、截图引擎内 diff。clean-chrome 侧 REQ-015/ADR-0009 逐批落引擎，browse 侧跟薄封装。

## Criteria

- [x] 批 1 semanticSnapshot 引擎侧落地（clean-chrome 2da7694..d1d2517 十跑构建双态验收绿）加 browse 侧薄封装（Browse.semanticSnapshot 直调，不支持时报错带回退口径）
- [ ] 批 2 subscribeChanges（引擎侧 clean-chrome 排期中）
- [ ] 批 3 waitForResponse 引擎侧
- [ ] 批 4 screenshotDiff 引擎侧
