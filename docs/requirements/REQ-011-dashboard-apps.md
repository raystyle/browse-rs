---
id: REQ-011
title: 看板与应用生态批（issue #54/#55）
status: draft
priority: should
trace: null
---

# REQ-011：看板与应用生态批（issue #54/#55）

## Scenario

多实例并行作业只有文本 status；应用生态缺统一插件契约。

## Criteria

- [x] #54 只读看板：GET /（HTML 单页零外部资源）与 GET /dashboard/sse（2 秒帧 health 快照，EventSource 原生重连）；看板无写路由（不是工作 tab 的铁律由路由面佐证）
- [ ] #55 应用插件契约：apps 子命令路由与 ctx 安全面
