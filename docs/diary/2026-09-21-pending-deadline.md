# 2026-09-21 #52 批（pending 清登记窄接口与 exit 2 收口）

背景：workspace 技能层批评审（G12）暴露的既有缺陷单：外层 `tokio::time::timeout` 包 `session.call` 会把内层超时清登记路径一起 drop，真挂死页（响应永不到）留 pending 僵尸；skills.rs 探测腿（8 秒）与 semantic.rs Input 派发短超时两处同型。同批收口上批评记的 G-F（用法错 exit 2 文案与实际分叉）与 #49 随记（fetch CTA 指日志）。

## 修法（窄接口而非 Drop 补偿）

- cdp `send_with` 参数化 deadline；新增 pub `call_with_deadline(method, params, deadline)`（守卫 + 提交屏障 + 路由全同 call()，仅 deadline 内层化）；`pending_len` 诊断面（doc-hidden）供验收断言
- Drop 补偿被否：tokio Mutex 在 Drop（sync 上下文）里拿锁只能 try_lock，竞争窗漏清；窄接口复用既有 30 秒路径的成熟清理语义，零新机制
- 两处同型点切窄接口；文档明写「不要在外面再包 timeout」的边界

## e2e 验收（issue 验收 2 的锁）

cookie getter 死循环页（`Object.defineProperty(document,'cookie',{get(){while(true){}}})`）让探测 IIFE 永挂：goto 在 8 秒档返回（7 至 25 秒窗断言，过快=没等过慢=退 30 秒档）、零 page 键静默降级、`pending_len()==0`、browser 级调用健在。解楔两踩：Page.crash 与 navigate 都是页内 session 面，楔死渲染器连 crash 命令都不收（实测 30 秒超时）；正道是 browser 级 newTab 切走留楔死 tab 后台（后续测试全在活动 tab，teardown 连带收割）。tabs 段的 t1 挑选对多一个后台 tab 免疫（只做 browser 级操作）。

## 边界留档

server.rs 求值 300 秒外层超时 drop 复合求值（host.eval 内含多条 call）属同型的粗粒度边角：迟到的响应会被 route 的 take 清掉，只有真挂死才积尸，且单飞槽语义下求值超时本身罕见；不在本批扩面。用法错 exit 2 直出四处（next 闭包缺值、--port 非数字、workspace site/page 缺参），bail_arg 族口径统一。[实证: fmt、clippy -D warnings、test --workspace 全绿、doc、aidoc strict、E2E 3 测 74.83s（含挂死探测与复活链）、PEVO]

## 封版 v0.12.1

修复批取 patch（REQ-004 判据行）；#52 关单走 omc 委托道。
