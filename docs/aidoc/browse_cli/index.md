# browse-cli 0.21.0

browse CLI 的可测半边：daemon 客户端（HTTP 调用 + 自动拉起）。

bin 侧（`main.rs`）只做接线：手写参数解析、求值形态（`--eval` 显式 /
stdin 管道批处理 / `--repl` 显式交互；裸调用出本仓帮助体）
与 up/down/status 生命周期子命令。
会话状态（方言变量、活动 tab、引擎）全在 daemon 侧持久，
CLI 进程本身无状态、即起即走。

## Modules

- [`client`](client.md): daemon 客户端：本地 HTTP 调用 + 首次使用自动拉起 detached daemon。
- [`fetch`](fetch.md): 一次性只读抓取（#50/#56）：HTTP 优先，三条件升级引擎，markdown 直出。
- [`ledger`](ledger.md): 账本薄适配层（REQ-063；总台修正令 2026-09-20 收口）：签名道与只增面
- [`render`](render.md): 求值结果的打印面：大值自动落盘（artifact/checkpoint 的降级形态，

