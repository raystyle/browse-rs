# cdp::methods

CDP 命令方法清单与相近建议。

`methods.txt` 由 `tools/gen-cdp-methods.py` 从上游
`browser_protocol.json` + `js_protocol.json` 生成（652 条，跳过 events
与 redirect 别名，对齐 browser-harness-js `sdk/gen.ts` 口径），随源码
入库；升协议版本后重跑生成脚本再提交。

用途是**被动增强**：不拦截未知方法（避免清单滞后误伤真方法），只在
CDP 报 `not found` 时给相近建议，以及给方言 `cdpMethods(domain)` 做
运行时探针（对齐 bh 的 `Object.keys(session.Network)`）。

## Functions

- `method_exists` — 判断方法是否为清单内已知命令（被动增强用，不预拦未知方法）。
- `methods_of_domain` — 返回某域的全部命令，是 `cdpMethods("Network")` 的底座。
- `suggest` — 相近建议：同域优先，方法名前缀/包含次之，最多 `max` 条。

## Constants

- `METHODS_RAW` — 全量 CDP 命令清单，`Domain.Method` 每行一条（652 条）。

