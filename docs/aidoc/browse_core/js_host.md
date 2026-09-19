# browse-core::js_host

方言求值器：把 [`crate::parser`] 的语句树跑在宿主侧。

移植自 browser-harness-rs `src/js_host.rs` 的求值半边：

- `session.<Domain>.<method>(params)` 直接转发 CDP 字符串调用（无 652 个
  typed wrapper，新 Chrome 方法不用 codegen）。
- 宿主全局：`listPageTargets()`、`resolveWsUrl(opts?)`、`print(x)`。
- `session.connect / use / waitFor / call / isConnected / getActiveSession`。
- `vars` 在 daemon 内跨片段持久（`const tabs = ...` 之后的片段还能用 `tabs`）。

## Functions

- `base64_decode` — 标准字母表的 base64 解码，容忍空白，不引 crate。
- `load_secrets` — 加载 dotenv 形密钥文件（#25.4）：`KEY=VALUE` 行，`#` 注释与空行忽略，
- `mask_secrets` — 对值做脱敏（#25.4）：字符串里出现任何密钥值即整值换 `***`（保守全换，
- `mask_secrets_str` — 对错误/回显串做子串脱敏（#25.4 评审 G4）：只换密钥值出现处，保留
- `render_result` — 把方言求值结果渲染成 CLI stdout 文本：字符串带引号（JSON 转义，与对象

## Types

- `JsHost` — 方言宿主：持有一个 CDP [`Session`] 加一份跨片段持久的变量表。

