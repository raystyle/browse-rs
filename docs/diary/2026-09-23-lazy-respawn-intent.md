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
