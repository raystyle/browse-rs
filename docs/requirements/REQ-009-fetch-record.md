---
id: REQ-009
title: 抓取与录制批（issue #50/#43）
status: draft
priority: should
trace: null
---

# REQ-009：抓取与录制批（issue #50/#43）

## Scenario

agent 要「拿一页正文」也得走全驾驶流；录制回放看不清点了哪里。#50 补一次性只读快路径，#43 补录制可读性。

## Criteria

- [x] #50 browse fetch：HTTP 直取加三条件升级引擎（空、墙词、薄内容；分类器单测锁）加页内抽取直出（v1 启发式非 Readability 已披露）
- [ ] #43 录制增强：光标轨迹、章节标记、动作标注（转代码二期）
