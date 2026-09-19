---
id: REQ-011
title: 看板与应用生态批（issue #54/#55）
status: implemented
priority: should
trace: apps/web-fetch 实弹三验（复用 daemon、崩溃边界、跑通）；apps.md 契约档
---

# REQ-011：看板与应用生态批（issue #54/#55）

## Scenario

多实例并行作业只有文本 status；应用生态缺统一插件契约。

## Criteria

- [x] #54 只读看板：GET /（HTML 单页零外部资源）与 GET /dashboard/sse（2 秒帧 health 快照，EventSource 原生重连）；看板无写路由（不是工作 tab 的铁律由路由面佐证）
- [x] #55 应用插件契约：apps/web-fetch 样例（pwsh 载体，browse fetch 薄封装）加 docs/guides/apps.md 契约五条（路由、ctx 只读面、helper 走 #44 snippets 覆盖、BROWSE_NAME 隔离、崩溃边界）；实弹三验：复用默认 daemon、崩溃 exit 1 后 daemon 仍活、样例跑通
