# ADR-0001：常驻 daemon，CLI 首次使用自动拉起

- Status: accepted
- Date: 2026-09-14
- Deciders: 用户裁定（CLI 形态问询）+ 维护者

## Context

消费者是编码 agent：每次 `browse '<片段>'` 都是一个短命进程。若无常驻层，
每个进程要重新连 CDP、重 attach tab、重跑前置片段；样例 browser-harness-rs
的 `--serve` 已证明 daemon 形态可行，但要求手动启动，且一次性模式每进程重连。
bh（Node 平台）的成功形态同样是「CLI -> 常驻 daemon -> 单 WS」。

## Decision

`browse` CLI 探活 `GET /health`（400ms），不通就 detached 拉起
`browse --serve`（日志 `%USERPROFILE%\.browse-rs\daemon.log`），轮询 10 秒就绪。
daemon 持有 Session（CDP 通道）、JsHost 变量表、引擎状态；
`browse down` 退出。因为会话状态（vars、活动 tab、引擎）必须跨进程存活，
所以生命周期必须由 daemon 而非 CLI 进程承载。

## Consequences

- 好：一条命令即用（零仪式感）；变量与活动 tab 跨调用持久；引擎复用免冷启动。
- 好：daemon 是唯一 CDP 客户端，守卫与事件缓冲只实现一份。
- 坏：多一层本地 HTTP 面（仅 127.0.0.1:9880）；daemon 挂了要靠 CLI 自动重生，
  长跑泄漏风险靠事件环形上限兜底。
- 坏：并发片段排队（单飞槽）而非并行；对单 agent 场景可接受。

## Alternatives

- 样例的纯一次性模式：每进程重连重连 CDP，状态无法跨调用，agent 体验差。
- 每用户常驻系统服务：部署复杂度高，v0.1 无需求支撑。
