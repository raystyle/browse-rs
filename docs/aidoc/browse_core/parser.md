# browse-core::parser

browser-harness-js 片段方言的语法分析器（纯函数）。

移植自 browser-harness-rs `src/js_host.rs` 的解析半边，独立成模块便于单测。
方言支持：字面量、对象、数组、成员、下标、`await`、`const/let/var`、
`return`、`//` 注释。不支持：函数字面量、`if/for/while`、模板字符串——
这些在解析期就报错并提示「页面逻辑放 `Runtime.evaluate` 的 expression」。

## Functions

- `parse_script` — 解析整段片段为语句列表（先剥 `//` 注释）。
- `render` — 语句列表回显成源码（诊断与 doctest 用，非规范格式化器）。
- `snippet_complete` — 片段是否括号配平（stdin/TTY 增量读入用：配平才送求值）。
- `strip_comments` — 剥掉 `//` 行注释（保留字符串字面量里的 `//`）。

## Types

- `Expr` — 表达式节点。
- `Stmt` — 语句：声明、表达式或 return。

