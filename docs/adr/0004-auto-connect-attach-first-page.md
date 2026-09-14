# ADR-0004：自动连接并 attach 首个 page target

- Status: accepted
- Date: 2026-09-14
- Deciders: 维护者

## Context

样例方言要求片段先 `session.connect(...)` 再 `session.use(targetId)` 才能
碰 Page/Runtime 域。对 agent 而言每条片段都重复这两步是纯摩擦；
bh 的做法是 daemon 自动 attach 第一个可用页面（attachFirstPage +
ensure_real_tab：没有真实页面就开 about:blank，不附着空目标）。
但显式 `session.connect({port:...})`（换浏览器）必须仍然有效。

## Decision

daemon 求值前的懒引擎：未连接且片段不含 `connect` 时 `Engine::ensure`；
ensure 成功后 attach 首个 page target（无则 `createTarget("about:blank")`）。
片段显式 `session.connect`/`session.use` 不受影响（预连被跳过；
求值撞 "Not connected" 时兜底 ensure 后整段重试一次）。
因为 CDP 的非 browser 域调用都路由到活动 sessionId，没有活动 tab 的
会话对 agent 等于不可用，所以 attach 是连接语义的一部分而非可选便利。

## Consequences

- 好：`browse 'await session.Page.navigate(...)'` 一条即通。
- 好：ensure_real_tab 契约避免附着 omnibox/后台空目标（list 滤 chrome://）。
- 坏：`contains("connect")` 的字符串嗅探是启发式：片段先写别的再 `connect`
  会触发一次无效预连然后由片段自己重连（兜底重试保正确性，代价是多一次连接）。
- 坏：agent 想显式选择目标 tab 时，自动 attach 的首 tab 只是起点，仍要 `use`。

## Alternatives

- 引入全局 `--no-auto-attach` 旗标：v0.1 无需求，YAGNI。
- 完全显式（样例行为）：每片段两句前置，摩擦大。
