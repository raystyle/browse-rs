# tools 清单

> 维护脚本归档：存量 Python 走 uv（PEP 723 零依赖）不迁移；验收与运维面新增一律 pwsh 7（用户定调 2026-09-16）。脚本产物是生成物的，源头 refs/ 不入库，改协议版本重跑再提交。

| 脚本 | 用途 | 产物 |
|---|---|---|
| [gen-cdp-methods.py](gen-cdp-methods.py) | 从上游 browser_protocol.json + js_protocol.json 生成 CDP 命令清单（口径对齐 browser-harness-js sdk/gen.ts：跳过 events 与 redirect 别名） | `crates/cdp/src/methods.txt`（入库） |
| [release.pwsh](release.pwsh) | 本地发布面（build-release 标准三段式前两段）：版本闸加测试闸加三目标编译（mac 实机）加打包边车加跨宿主断言与解包冒烟加 gh release 直发 --latest | GitHub Release 六件（镜像播种由 CI 流水接力） |
