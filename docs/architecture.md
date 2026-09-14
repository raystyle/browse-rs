# architecture：现在怎么拼

> 描述**现在**的结构与数据流；历史决策在 `docs/adr/`。本文随结构变化同 PR 更新。

## 三层 crate

```
crates/cdp          连接与协议（无业务语义）
  session.rs        一条 CDP 会话：browser-level WS 或 S005 管道；flatten attach；
                    sessionId 路由；事件环形缓冲（1000 条）；call 守卫
  discovery.rs      wsUrl/port/profileDir -> WS URL；probe_default 附着探测；
                    DevToolsActivePort 文本解析（纯函数，契约测试锁）
  spawn.rs          chrome 发现序；端口态/管道态 spawn；terminate_pid 兜底
  pipe.rs           匿名管道薄封装（Windows CreatePipe；POSIX 桩）

crates/browse-core  语义与 daemon（不碰 argv）
  parser.rs         方言语法分析器（纯函数；方言外语法解析期报错并给提示）
  js_host.rs        方言求值器：session.<Domain>.<method> 直转 CDP；
                    vars 跨片段持久；全局 listPageTargets/resolveWsUrl/
                    detectBrowsers/print；session.close/setActiveSession/
                    事件家族 peekEvents/peekEventsSince(seq 游标增量)/
                    findEvents(点分路径等值过滤)
  engine.rs         引擎状态机：附着优先缺则自起；只杀自己 spawn 的；
                    auto attach 首个 page target
  server.rs         daemon HTTP API：/eval（单飞槽 + 懒引擎 + 超时）、
                    /health、/engine/up、/quit

crates/browse-cli   bin 只接线
  client.rs         daemon 客户端：探活、detached 自动拉起、eval/up/quit
  main.rs           手写参数解析（无 clap）；三传输形态（--eval/stdin/TTY）；
                  up/down/status 子命令；--pipe/--headless/--ws/--port/--chrome
```

## 一次 `browse '<片段>'` 的数据流

```
CLI 探活 GET /health（400ms）
  不通 -> detached 拉起 `browse --serve`（日志 %USERPROFILE%\.browse-rs\daemon.log）-> 轮询就绪
POST /eval {code}
  daemon：
    未连接且片段不含 "connect" -> Engine::ensure（显式 ws/port -> probe_default -> spawn）
    求值撞 "Not connected" -> ensure 后整段重试一次
    JsHost 求值：parser 解析 -> 逐语句求值 -> session.call（守卫 + sessionId 路由）
      -> CDP 通道（WS Text 帧 或 管道 ASCIIZ NUL 帧）-> chrome
  响应 {ok, value} -> CLI render_result 打印
```

## CDP 双通道（同一条 Session）

| | WS（端口态） | 管道态（--pipe） |
|---|---|---|
| 建立 | `/json/version` 或 `DevToolsActivePort` -> ws connect | spawn 传 `--remote-debugging-io-pipes=<in>,<out>`（句柄可继承）+ `CLEAN_CHROME_DEBUG=pipe` |
| 就绪信号 | 轮询端口文件（15s 上限） | 无需探测，首条调用即等待 |
| 帧 | WebSocket Text | NUL 分隔 JSON（ASCIIZ） |
| 断线 | `connected=false` | 同左；clean-chrome 侧断管自关浏览器 |
| TCP 面 | 127.0.0.1 端口 | 零 |

两通道共用 pending 应答表、事件环形缓冲、守卫与 `sessionId` 注入；只有字节泵不同（`open_ws` / `connect_pipes`）。

## 守卫（cdp::Session::call 层，程序级强制）

- `Browser.close` / `Browser.setWindowBounds`：一律拒绝（错误串给下一步指令）。
- `Target.closeTarget`：只放行本会话 `Target.createTarget` 产物（自建 tab 集合）。
- 引擎优雅退出唯一旁路：`Session::graceful_close_browser`，仅 `Engine::shutdown` 对 spawn 来源调用。

## 测试面

- 单元/契约（无网络）：parser 语法矩阵、discovery 纯解析、Session::new 状态。
- doctest：各 crate `///` 示例（可跑的带 assert）。
- E2E（`BROWSE_E2E=1` 门控，CI 跳过）：双通道各一：spawn headless -> navigate data: URL -> title 断言 -> 守卫断言 -> shutdown；测试间用互斥锁串行（engine-profile 独占）。
