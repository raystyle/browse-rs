# browse-core::js_host

方言求值器：把 [`crate::parser`] 的语句树跑在宿主侧。

移植自 browser-harness-rs `src/js_host.rs` 的求值半边：

- `session.<Domain>.<method>(params)` 直接转发 CDP 字符串调用（无 652 个
  typed wrapper，新 Chrome 方法不用 codegen）。
- 宿主全局：`listPageTargets()`、`resolveWsUrl(opts?)`、`print(x)`。
- `session.connect / use / waitFor / call / isConnected / getActiveSession`。
- `vars` 在 daemon 内跨片段持久（`const tabs = ...` 之后的片段还能用 `tabs`）。

## Functions

- `render_result` — 把方言求值结果渲染成 CLI stdout 文本：标量裸打、空容器不打、其余打 JSON。

## Types

- `JsHost` — 方言宿主：一个 CDP [`Session`] + 一份跨片段持久的变量表。

