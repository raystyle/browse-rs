# 2026-09-23 #60 批（操作回执的引擎上下文 CTA）

背景：#59 的同日姊妹单（2026-09-22 跨宿主混淆事故）：status 是查询面，操作时刻才是事故现场：那天 fetch 与片段命令无感知走了 WSL 侧 daemon 与引擎，回执零引擎上下文。#59 批已落 provenance 地基（v0.18.0），本批把它消费到操作面。

## 修法（信封键，一份事实两处展示）

- server.rs：eval 信封加 engineContext 键（provenance 加 daemon 自描述同源 #59，加 changes 数组；方言 value 不动向后兼容）；Daemon 加 last_fp 指纹存储（Mutex<Option<Value>>），变化检测在 daemon 侧逐操作比对，CLI 无状态
- engine.rs：fingerprint_of（origin/pid/有头无头/通道/profile 形态/附着 host，hostContext 与实例名在 daemon 生命周期内恒定不参与逐操作比对）加 fingerprint_changes（差异列人读句，键集不同按有无出句不 panic）；Engine::context_snapshot 一把出 provenance 加 fingerprint
- client.rs：render_engine_context（eval 与 fetch 引擎腿共用）：正常态零输出（技能触发层未命中零新增键先例），异常出 stderr 告警行；跨宿主与同 OS 异机告警每进程一次（OnceLock 防批处理刷屏）；BROWSE_ENGINE_CONTEXT 开紧凑行（引擎：origin @ hostContext）
- 披露：surface env 块与 README env 表加 BROWSE_ENGINE_CONTEXT 行

## 实证

单测 fingerprint_diff_shapes（无变化空 vec、pid 换新、有头无头翻转、形态翻转含 origin 同随、附着切换键集不同）。实弹：daemon 存活期 kill -9 引擎逼懒重 spawn，次发操作精确告警「引擎换新：2240203 -> 2240420」加「有头无头翻转：无头 -> 有头」，顺带照出真缺陷：懒重 spawn 用 daemon 存的缺省 spec（headed），丢 up --headless 意图（另立 issue）；紧凑行开关实弹过。[实证: fmt、clippy -D warnings、test --workspace 13 套、doctest、cargo doc、aidoc --check --strict、surface_contract 8、PEVO、e2e 真 chrome 4 passed 随批跑]

## 封版 v0.19.0

能力新增取 minor（REQ-004 判据行）；评审轮与发版随后补记。

## 评审一轮（browse-codex-review）

四点名边界过（信封兼容、锁面、OnceLock 语义、键集边界）。F 两条必修：F1 diary 三处破折号踩 PEVO 禁字且回执自述与实况矛盾（#56 批同款「写完 diary 未复跑 PEVO」重演，已修三处标点并把「PEVO 最后跑」内化为批纪律）；F2 http-api.md /eval 契约段补 engineContext 键说明（#59 轮 F3 同款口径：契约档须随批）。G1 采纳：origin 变化时键集差异是形态切换副产物不是独立信号（附着态没有有头无头概念），只出来源句不稀释真信号；G2 采纳：origin 与逐键字符串值走裸值不带 JSON 引号。G1 后实弹复核：同 origin 真翻转双句（引擎换新加有头无头）零噪声；首发 eval 无基线零告警（语义正确，kill 前先存基线才有比对面）。issue #61（懒重 spawn 丢 headless 意图）随批入账。
