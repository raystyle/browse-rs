# architecture：现在怎么拼

> 描述**现在**的结构与数据流；历史决策在 `docs/adr/`。本文随结构变化同 PR 更新。

## 三层 crate

```
crates/cdp          连接与协议（无业务语义）
  session.rs        一条 CDP 会话：browser-level WS 或 S005 管道；flatten attach；
                    sessionId 路由（call_on 可显式指定目标 session）；事件环形
                    缓冲（1000 条，seq 盖戳）；call 守卫；drain_events 消费式取；
                    连接态状态机（conn_epoch 纪元：重连清 pinned/doc_gens/
                    屏障/在途 pending，旧泵静默；close() 同律 drain；文档代
                    计数与提交屏障见 js_host 条目；waitFor 活动过滤。提交屏
                    障是弱保证（#57 G1）：水位取自活动 session 的 send 前
                    seq，回执到达前换靶会把水位挂到新 sid（白等满窗后保守
                    放行留痕）；屏障入口不做 seen-since 过滤，提交信号早于
                    水位即放行——两个方向都偏保守不偏漏）
  discovery.rs      wsUrl/port/profileDir -> WS URL；probe_default 附着探测；
                    DevToolsActivePort 文本解析（纯函数，契约测试锁）
  spawn.rs          chrome 发现序；端口态/管道态 spawn（Windows 句柄继承 /
                    POSIX pre_exec dup2 到 fd 3/4）；terminate_pid 兜底
  pipe.rs           匿名管道薄封装（Windows CreatePipe；POSIX pipe(2)）
  methods.rs        652 命令清单（生成物）+ 拼写建议

crates/browse-core  语义与 daemon（不碰 argv）
  parser.rs         方言语法分析器（纯函数；方言外语法解析期报错并给提示）
  js_host.rs        方言求值器：session.<Domain>.<method> 直转 CDP；
                    vars 跨片段持久；元素引用表（snapshot 短 ref +
                    daemon 侧文档代：session 与导航计数，页面零写入，
                    #30 消注入痕）；全局函数族（见 README 清单）
  semantic.rs       语义层：tab 族、交互三件（clickAt/fillInput/pressKey）、
                    clickRef/fillRef（backendNodeId 锚）、等待判官
                    （waitLoad/waitIdle）；Input 挂起自愈重试；goto 回执
                    构建后经 skills::augment_goto 条件附技能键
  skills.rs         技能触发层（#50/#51，ADR-0008）：goto 后按域名段点名
                    workspace 站点知识、按页面特征（一次有界 pageEval，
                    10 slug 两档置信）点名机制配方；未命中零新增键
  record.rs         录制：startScreencast 帧流泵落盘 + 按 sessionId ack
  paths.rs          实例命名空间（BROWSE_NAME -> 状态目录 + 派生端口，
                    ADR-0006）；engine_profile_dir 与 workspace_dir（跨
                    实例共享的知识仓根）等落点
  workspace.rs      workspace 单仓管理（git shell-out）：clone/pull 与
                    仓内文件读取（site/page 的越界守卫），CLI 直读不经
                    daemon
  engine.rs         引擎状态机：附着优先缺则自起（BROWSE_NO_ATTACH 可跳过
                    探测）；只杀自己 spawn 的；auto attach 首个 page target
  server.rs         daemon HTTP API：/eval（单飞槽 + 懒引擎 + 超时）、
                    /health、/engine/up、/quit

crates/browse-cli   bin 只接线
  client.rs         daemon 客户端：探活、detached 自动拉起、eval/up/quit；
                    端口随 BROWSE_NAME/BROWSE_PORT
  render.rs         打印面：大值自动落盘（>32KB 写 drops/，stdout 回
                    __dropped 提示行 + 预览）
  main.rs           手写参数解析（无 clap）；三传输形态（--eval/stdin/TTY）；
                    up/down/status 子命令；--pipe/--headless/--ws/--port/--chrome
```

## 一次 `browse '<片段>'` 的数据流

```
CLI 探活 GET /health（400ms）
  不通 -> detached 拉起 `browse --serve`（日志 <state>/daemon.log）-> 轮询就绪
POST /eval {code}
  daemon：
    未连接且片段不含 "connect" -> Engine::ensure（显式 ws/port -> probe_default -> spawn）
    求值撞 "Not connected" -> ensure 后整段重试一次
    JsHost 求值：parser 解析 -> 逐语句求值 -> session.call（守卫 + sessionId 路由）
      -> CDP 通道（WS Text 帧 或 管道 ASCIIZ NUL 帧）-> chrome
  响应 {ok, value} -> CLI render_or_drop：小值直打，超 32KB 落盘回 __dropped 行
```

## CDP 双通道（同一条 Session）

| | WS（端口态） | 管道态（--pipe） |
|---|---|---|
| 建立 | `/json/version` 或 `DevToolsActivePort` -> ws connect | Windows：句柄可继承 + `--remote-debugging-io-pipes=<in>,<out>`；POSIX：pre_exec `dup2` 布到 fd 3/4；都带 `CLEAN_CHROME_DEBUG=pipe` |
| 就绪信号 | 轮询端口文件（15s 上限） | 无需探测，首条调用即等待 |
| 帧 | WebSocket Text | NUL 分隔 JSON（ASCIIZ） |
| 断线 | `connected=false` | 同左；clean-chrome 侧断管自关浏览器 |
| TCP 面 | 127.0.0.1 端口 | 零 |

两通道共用 pending 应答表、事件环形缓冲、守卫与 `sessionId` 注入；只有字节泵不同（`open_ws` / `connect_pipes`）。POSIX 分支经 WSL 真 Linux 编译+单测，CI ubuntu 持续门禁（ADR-0005）。

## 多实例（BROWSE_NAME，ADR-0006）

一个名字同时决定状态目录（daemon 日志/drops/screenshots/录制/engine-profile）
与 daemon 端口（fnv1a 派生 9900-9999；`BROWSE_PORT` 显式优先）。默认实例
零变化。命名实例各自 spawn chrome（profile 独占不互锁）。

## workspace 技能仓（ADR-0008）

知识仓 `github.com/raystyle/browse_workspace` 部署在 `~/.browse-rs/workspace`
（`BROWSE_WORKSPACE` 覆盖；**不分 BROWSE_NAME**，跨实例共享）。三层使用面：
goto 回执自动点名（domain_skills/page_skills 加 hint，`BROWSE_DOMAIN_SKILLS=0`
/`BROWSE_PAGE_SKILLS=0` 分层关）-> `browse workspace site/page` 免浏览器读全文
（CLI 直读文件，canonicalize 越界守卫）-> `browse workspace list` 意图反查。
install/update 走 git shell-out（clone / pull --ff-only 脏树拒绝），本地修改
手工 commit/push 回推。env 三变量经 `ensure_daemon_with_env` 显式透传（防御性：
现状与继承等效，防未来 env_clear 与非 CLI 拉起路径；改配置重启 daemon，
BROWSE_SECRETS 同口径）。

## 守卫（cdp::Session::call 层，程序级强制）

- `Browser.close` / `Browser.setWindowBounds`：一律拒绝（错误串给下一步指令）。
- `Target.closeTarget`：只放行本会话 `Target.createTarget` 产物（自建 tab 集合）。
- 引擎优雅退出唯一旁路：`Session::graceful_close_browser`，仅 `Engine::shutdown` 对 spawn 来源调用。

## 测试面

- 单元/契约（无网络）：parser 语法矩阵、discovery 纯解析、paths 端口派生、
  Session 事件家族/守卫 CTA、POSIX 管道往返（unix）。
- doctest：各 crate `///` 示例（可跑的带 assert）。
- E2E（`BROWSE_E2E=1 BROWSE_NO_ATTACH=1` 门控，CI 跳过）：双通道各一：
  spawn headless -> navigate/事件家族/waitJs/snapshot/元素引用（含失效 CTA）/
  录制/语义面全链 -> 守卫断言 -> shutdown；测试间互斥串行（profile 独占）。
