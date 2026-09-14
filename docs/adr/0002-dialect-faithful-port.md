# ADR-0002：方言忠实移植，不做子命令操作面

- Status: accepted
- Date: 2026-09-14
- Deciders: 用户裁定（AskUserQuestion 三选一：子命令+逃生舱 / 忠实移植方言 / 两者都要）

## Context

Rust 没有免费 V8。bh（Node）靠 daemon 里的真 V8 跑片段；样例 browser-harness-rs
为此手写了 JS 子集解释器（字面量/对象/成员/下标/await/const-let-var/return，
无 if/for/函数/模板字符串）。备选是子命令 CLI（browse goto/js/cdp），
参数结构化、无引号地狱，但与样例形态分叉。

## Decision

忠实移植样例方言与三传输形态（`--eval` / stdin 括号配平 / TTY REPL），
不做子命令操作面（仅 up/down/status 生命周期词）。方言外的语法在解析期
报错并提示「页面逻辑放 Runtime.evaluate 的 expression」：复杂逻辑本就
属于页内真 V8。因为样例方言已被验证够用、且解释器（parser.rs 纯函数）
可被契约测试完全锁住，所以移植比重新发明更便宜。

## Consequences

- 好：与样例/上游片段语义一致，样例 README 的例子原样可跑。
- 好：parser 是纯函数，错误矩阵进 `tests/parser_contract.rs`，无 I/O 依赖。
- 坏：方言表达力受限（无控制流）；agent 必须把逻辑拼进 expression 字符串，
  引号嵌套偶有摩擦。
- 坏：解释器是长期维护面（虽小）。

## Alternatives

- 子命令 + `browse js`/`cdp` 逃生舱：agent 更友好，但与用户裁定相悖，留作未来演进（见 README Roadmap）。
- 嵌 V8（deno_core/v8 crate）：依赖树与构建成本暴涨，远超 v0.1 体量。
