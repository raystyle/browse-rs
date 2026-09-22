# 2026-09-23 #61 批（懒重 spawn 丢 headless 意图）

背景：#60 批验收实弹照出（issue #61 当场入账）：browse up --headless 起无头引擎后 kill -9 引擎 pid，daemon 懒重 spawn 走 daemon 启动时存的 from_env 缺省 spec（headless 缺省 false），新引擎回有头（有显示环境弹真窗）；#60 的有头无头翻转告警把这一隐形变化当场曝光，本批修根。

## 修法

- server.rs：/engine/up 成功且 Auto（spawn）意图时回写 daemon 缺省 spec；spec 字段 Mutex 形（Daemon 在 Arc 后共享），eval 两读点克隆快照防长持锁；Attach 意图不回写（连接意图非重生意图）
- surface up 述补形态记忆口径（显式 up 的 spawn 形态被 daemon 记住，意外退出后的自动重拉沿用）

## 实证

修复前（#60 批实录）：重拉后双句告警（引擎换新加有头无头翻转），headless true 对 false。修复后：同场景仅「引擎换新」一句，headless True 对 True（status --json 断言）。[实证: fmt、clippy -D warnings、test --workspace 13 套、aidoc --check --strict、surface_contract、PEVO 随批跑；实弹见上]

## 封版 v0.19.1

修复批取 patch（REQ-004 判据行）；评审轮与发版随后补记。

## 评审一轮（browse-codex-review）

F1 必修（核实为真，走披露道收）：回写是整份 spec 覆盖，请求未给字段带显式默认（headless=false 等）而非「不修改」，裸 browse up 会静默复位前次形态记忆，与「会被记住」措辞不符。语义裁定：up 本就是「按这些参数起引擎」，最近显式 up 定义重拉形态、裸 up 即重定义，覆盖是对的、措辞是错的：surface 改「最近一次 up 的意图生效（裸 up 即复位为有头缺省）；记忆是 daemon 进程态重启回环境缺省；附着意图不记忆重拉走发现序」，并把裸 up 复位复现记档：up --headless 后裸 up，再 kill 引擎，重拉回有头（最近显式意图是裸 up 的 false）。G1（附着面记忆缺角）与 G2（进程态非持久化）一并收进同句披露。合并道（三态化缺省不等于 false）记档不做：动 /engine/up 线上契约收益边际。

## 发版关单与 r5 验收（同日续）

- 推送 8fad6ba..6501cff（含评审三轮的披露收口：F1 披露道、getting-started 首次语义矛盾整句重写），CI 与播种绿；tag v0.19.1，五端拉平实弹毕；关单正典双事件（seq 221 result 加 222 status），台账再清零
- 过程记档：本批自己踩尾管吞 PEVO 红坑（第三次），已改按退出码判（cmd > f; S=$? 形）；评审方同坑提醒在案
- r5 验收（用户令「都做 验收」，随 clean-chrome r5 窗）：四件全绿。镜像装（1.9GB/567 文件锚校验）、绑定生效（BROWSE_ENGINE_ARGS 透传 --remote-debugging-address=0.0.0.0 后 ss 实证 LISTEN 0.0.0.0:42275，r4 同形 127.0.0.1）、mesh 直暴真驱动（lan-ubuntu 冷 daemon --connect ws://10.10.10.5:42275 驱动 wsl r5 引擎，goto 339ms，wsl 活动 tab 被远程改写坐实；#59 provenance 亮 attached 10.10.10.5）、缺省回归（无旗标仍 127.0.0.1）。验收回执已发 clean-chrome 工位闭环，omc 滚 chrome latest 指针随行
- 附坑记档：--connect 对已有引擎的 daemon 是幂等静默（不切换附着目标），冷 daemon（down 后首调）才真附着：本日 mac 串台误判与本次 lan-ubuntu 首连假绿同根，候选入 issue 面
