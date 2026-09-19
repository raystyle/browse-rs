---
id: REQ-009
title: 抓取与录制批（issue #50/#43）
status: implemented
priority: should
trace: #50 分类器单测加实弹；#43 实弹双开录加章节落盘；e2e 52.38s
---

# REQ-009：抓取与录制批（issue #50/#43）

## Scenario

agent 要「拿一页正文」也得走全驾驶流；录制回放看不清点了哪里。#50 补一次性只读快路径，#43 补录制可读性。

## Criteria

- [x] #50 browse fetch：HTTP 直取加三条件升级引擎（空、墙词、薄内容；分类器单测锁）加页内抽取直出（v1 启发式非 Readability 已披露）
- [x] #43 录制增强 v1：cursor 光标元素、showActions 点击闪圈、recordChapter 章节落盘（转代码二期记档 surface 与本 REQ）
