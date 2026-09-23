# 2026-09-23 #62 批（显式连接意图强制切换）

背景：用户令「解决啊」（前批附坑记档的 --connect 幂等静默，本日两踩同根：lan-mac 串台误判差点冤案 clean-chrome 指纹抹平、lan-ubuntu 首连假绿靠 wsl tab 复核识破）。issue #62 先入单再修。

## 修法

- engine.rs：ensure 首闸分流：显式意图（Attach/Port，来自 --connect/--ws/--port/BROWSE_CDP_WS）目标不同即切换；同目标幂等免抖（ws_port_of 端口比对，BROWSE_CDP_WS 钉死 daemon 逐 eval 不致次次断重连）。切换处置：旧引擎 spawn 的走 shutdown 优雅关（down 所有权加隔离目录清理），attached 的 session.close 零触碰只断本侧；Auto 缺省意图维持原幂等（懒起与发现序不动）

## 实证

- e2e 新增 explicit_attach_switches_from_spawned_engine：双隔离引擎 A/B，A 暖态 Attach 到 B 的 ws 真切换（来源 Attached{B}）、A 优雅关（隔离目录随删）、同目标重复幂等；e2e 5 passed
- CLI 级暖态真火（本机双实例）：默认实例带自家引擎 A（暖），--connect 到 second 实例引擎 B 的 ws：goto 419ms 跑在 B、默认实例引擎态变 attached（#59 provenance 带 loopback 降级标注）、B 活动 tab 被改写坐实、旧 A 进程优雅关退场

## 封版 v0.20.0

行为变化取 minor（REQ-004 判据行）；评审轮与发版随后补记。

## 评审一轮（browse-codex-review）

四点名边界部分过。F 两条必修全实：F1 Port 同目标判只比端口不比宿主（远端同端口附着被误判同目标吞掉本机切换意图）；F2 Port 连接来源记「port N」合成串（ws_port_of 解不出致 Port 幂等失效每次断重连，attachedHost 显示同步受害）。修法：Port 臂先 discovery 解真 ws URL 再连、来源记真端点；同目标判加 is_loopback_host（从 provenance 内联判式抽出共用）；单测补 F1 远端不同目标与 F2 本机真 ws 同目标两断言。G1 采纳：Engine 加 switch_lock 单飞锁（ensure 全程持，防两并发显式切换互撕连接与来源记录；不能复用 inner 因 shutdown 要取它）；G2 采纳记档：宿主级 vars/refs 跨切换不清（与「变量跨调用持久」口径一致，diary 点明免误解）；G3 记档不做（ws 字面等值的 localhost/127 写法差异低频）。评审另正一笔：CHANGELOG/diary 的「BROWSE_CDP_WS 钉死 daemon 逐 eval 免抖」措辞不准，它守的是重复 up 不是逐 eval（eval 懒起只在未连接时走 ensure），已随批改准。

## 发版与关单（同日续）

- 推送 6a67571..b346102，CI 与播种绿；tag v0.20.0，五端拉平全镜像道；关单正典双事件（seq 225 result 加 226 status），板上转余 #63（用户新需求：workspace 多仓加载，已入单待开工）
