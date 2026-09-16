# tools 清单

> 维护脚本归档：uv 运行时 Python（PEP 723 零依赖）。脚本产物是生成物的，源头 refs/ 不入库，改协议版本重跑再提交。

| 脚本 | 用途 | 产物 |
|---|---|---|
| [gen-cdp-methods.py](gen-cdp-methods.py) | 从上游 browser_protocol.json + js_protocol.json 生成 CDP 命令清单（口径对齐 browser-harness-js sdk/gen.ts：跳过 events 与 redirect 别名） | `crates/cdp/src/methods.txt`（入库） |
