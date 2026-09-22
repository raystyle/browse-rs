# 2026-09-23 #59 批（引擎来源与所有权信息面）

背景：2026-09-22 跨宿主混淆事故的两单提案之一（#59 查询面地基，#60 操作告警面随后吃它）：同机同日引擎状态三变，CLI 只能靠 profile 路径人肉推断宿主；Windows CLI 经 localhost 转发命中 WSL daemon 时两边都是 127.0.0.1 零提示。前夜的远程操刀测试已补足实证材料（Attached 只有 ws_url 字面、loopback 不可辨、platform 指纹抹平）。

## 修法（一份 provenance 两处展示）

- engine.rs：EngineSource::Spawned 加 spawned_at（Unix 毫秒时间锚，序列化加法向后兼容）；provenance 纯函数产 {origin, hostContext, spawnedBy, spawnedAt} 四字段（origin 枚举 attached/managed-spawn/isolated-spawn；spawned 态引擎与 daemon 同宿主故 hostContext 权威；attached 态只取 ws host 字面，loopback 显式降级标注「本机或隧道不可辨」不冒充确定）；ws_authority_host 字面解析
- server.rs：DaemonDesc 自描述（os/hostname/pid/name/startedAt，hostname 走命令采值零新依赖）一次采值进 Daemon；health_json 内嵌 daemon 键与 engineProvenance 键（旧键全在）
- CLI print_health：人读面补「daemon 宿主」行与引擎行 @ origin hostContext since 段；CLI 与 daemon 跨宿主时显式告警行（cfg 比对 os）；hms_utc 纯除余时显
- surface status 述随卷；#60 将同源消费 provenance 结构（一份事实两处展示）

## 实证

单测：provenance 四态（托管/隔离 spawn 权威宿主、附着 loopback 降级、附着远端 host 字面、未连接）；health_json 装配面（daemon 四字段在位、未连接 origin=none、旧键全在向后兼容）。实弹：wsl 视角人读面全中（daemon 宿主 linux/AI-LAB 加 @ managed-spawn linux/AI-LAB since）；跨宿主数据面经 Windows CLI 实证（lan-win 0.17.0 经 localhost 转发取 wsl daemon 的 status --json，daemon.os=linux 与 provenance 四字段齐；告警渲染行待 0.18.0 拉平上 Windows 端即活）。[实证: fmt、clippy -D warnings、test --workspace、doctest、cargo doc、aidoc --check --strict、surface_contract、PEVO、e2e 随批跑]

## 封版 v0.18.0

信息面能力新增取 minor（REQ-004 判据行）；评审轮、发版与拉平随后补记。

## 评审一轮（browse-codex-review）

四点名边界过（含 #60 复用面判「够做形态翻转检测」）。F 三条必修全实：F1 doctest 补 u64 后缀后未重跑 aidoc（Must 违例，投影漂移精确一处），已 regen 随批；F2 ws_authority_host 对带括号 IPv6 按 `:` 误切（`[::1]` 得乱码 `[`），修为首 `]` 含括号整段加 IPv6 loopback 测试；F3 docs/guides/http-api.md 的 /health 契约段未随批（Spawned 枚举形过期加 daemon/engineProvenance 两键缺），已补全。G1 采纳（CLI os 判定换 env::consts::OS 同源，cfg 三态对 freebsd 类误兜底）；G3 采纳（同 OS 异机也告警：CLI 采本机 hostname 与 daemon.hostname 比对，隧道场景覆盖）；G4 半收（hms_utc 补日期走 civil-from-days 无闰表；pid 收进 provenance 归 #60 指纹批定）；G2 半收（hostname 采值超时记档下批；unknown 字面语义自明不加标注）。

## 发版与关单（同日续）

- 推送 2fcf5cf..df2ef70，CI 与播种全绿；tag v0.18.0，release.ps1 全链过，五端拉平全镜像道。验收 3 终极真火：Windows CLI 0.18.0 经 localhost 转发打 wsl daemon，跨宿主告警行当场显形（原始事故场景闭环）
- 关单走正典双事件（总台纠偏令：执行者归本工位，browse 键签名道，勿再转呈）：result 事件（seq 216，digest 锚 v0.18.0 linux 资产 sha256:04df01c6…，digest 用词总台转呈形）加 status 事件（seq 217 to=done）；一次性执行器复刻 ledger-client post_signed 签名道（七行基五头，密档 base64url seed，kid 5f6664e4…与 CLI 同源）。ledger 服务端 result 门槛实证在位（status to=done 无 result 即 400 指管理面 force-status）；browse CLI 的 close 面按 0.9.0 收口令维持移除，一次性执行器是密档键的正典签名道而非面回潮。回读自证：issues/59 projection=done（timeline 三事件），#60 复核 open 不动
