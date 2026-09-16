# browse-core::parser

browser-harness-js 片段方言的语法分析器（纯函数）。

移植自 browser-harness-rs `src/js_host.rs` 的解析半边，独立成模块便于单测。
方言支持：字面量、对象、数组、成员、下标、`await`、`const/let/var`、
`return`、`//` 注释。不支持：函数字面量、`if/for/while`、模板字符串；
这些在解析期就报错并提示「页面逻辑放 `Runtime.evaluate` 的 expression」。

## Functions

- `loc` — 把字节偏移换算成「行L:列C」（错误定位用，CTA 的一半是位置）。
- `parse_script` — 把整段片段解析成语句列表（先剥 `//` 注释）。
- `render` — 把语句列表回显成源码（诊断与 doctest 用，非规范格式化器）。
- `snippet_complete` — 判断片段括号是否配平（stdin/TTY 增量读入用：配平才送求值）。
- `strip_comments` — 把 `//` 行注释剥掉，保留字符串字面量里的 `//`。

## Types

- `Expr` — 方言的表达式节点，构成求值树。
- `Stmt` — 方言的顶层语句形态：声明、表达式或 return。

