# 2026-09-23 #58 批（update 下载腿进度面）

背景：v0.15.1 拉平夜的事故链（见 2026-09-22 两篇）：晚高峰镜像资产段爬速（根因是透明代理 h2 悬崖，omc 已在 openwrt 根除 offload 三关；browse 侧 3edf13f h1 钉死为纵深），但 `browse update` 下载腿在几分钟静默中零输出，多端体感「完全卡住」被误判死锁（issue #58，--dry-run 后实发）。本批把进度面补上。

## 修法（流式读加心跳）

- self_update.rs：资产体从一次性 `.bytes()` 改 `read_body_with_progress` 流式读（64KB 块），节流闸收满 512KB 或满 1 秒任一满足即回调（PROGRESS_MIN_BYTES/INTERVAL 常量）；fetch_pair 透传回调，fetch_asset 挂 eprintln 心跳（`下载中 N/总量（%）`，总量缺失降级字节式），取毕补收尾行；读失败错误带已收/总量/耗时上下文（超时可预期）；零新依赖；判新腿与镜像选序不动
- 边车与判新标记（单行小件）不走此道

## 测试与判据

慢源 mock（服务端按片写加片间 sleep，等价限速代理）驱直调：多拍心跳（300KB/8KB 片加 30ms 闸至少三拍）、每拍带总量且未到完成态、字节序单调、终值全量；断流 mock（声明全长只写半）驱错误上下文（资产名、已收/总量、耗时）。[实证: fmt、clippy -D warnings、test --workspace、doctest、cargo doc、aidoc --check --strict、surface_contract、PEVO 全绿随批跑]

## 封版 v0.16.0

能力新增取 minor（REQ-004 判据行）；0.15.2（h1 钉死加播种缓存头，总台令批）未单独发版，随本批一起出门。发版后五端 update 实弹即验收 4 的真源实弹（各端从 0.15.1 拉到 0.16.0 的下载心跳肉眼可见）。

## 评审与发版（随后补记）

评审轮、推送、v0.16.0 发版与五端拉平、#58 关单走 omc，随后补记。

## 评审两轮（browse-codex-review）

- 一轮无必修（四点名边界 CONFIRM：节流闸语义、完成态分工、断流上下文、透传影响面、版本判据 minor 维持）加 G1-G4：G2 镜像腿失败诊断透出、G3 预分配封顶 256MB、G4①③ 文档对齐与收尾行对称均随批修；G1（零字节 stall 静默需独立计时线程）记档备裁不做，300s 总超时兜底在册；G4②（缺 CL 完成拍语义）以文档对齐实况收
- 二轮 CONFIRM 放行。附言两条：256MB 帽可再收紧至 64MB 或去预分配（可选不动，记档）；eprintln 文案无测试锁记下批补强

## 发版与五端拉平（同日续）

- 推送 3edf13f..ad4bc0c（纯 diary amend 属评审方免重核口径），CI 三跑加播种全绿；tag v0.16.0，release.ps1 全链过，镜像 stable/latest 滚 0.16.0
- 五端拉平全镜像道（0.15.1 到 0.16.0；根除后镜像 2.8MB/s 级，秒级完成）。注记：本轮回执由 0.15.1 旧二进制执行故无心跳行可见，心跳面自下轮更新起实弹可见；验收 4 的慢源实弹已由节流 mock 测试锁
- #58 关单（omc，open 集清零）；catalog pin 滚 0.16.0（omc af74655 加 2079aee 已推，tools.toml 三平台 0.16.0 在册）；ark locked 0.11.0 属端上快照口径不并轨

## G 尾批（同日再续，0.17.0）

- 用户令随批两件：G1 stall 看门狗（零字节停顿每满 10 秒告警一行，独立线程监控共享进度态，读结束一节拍内自退不阻塞收尾）；chrome 下载同享进度面（「干脆chrome下载也做了这个特性」）
- 下载核心抽 stream_with_progress 通用缝：update 腿收 Vec、chrome install 腿写文件加哈希，心跳/看门狗/断流上下文同一口径；chrome 大件字节闸放宽 4MB（150MB 级包约 40 行）；文案抽 heartbeat_line/stall_line 纯函数带调用方 label（browse update / browse chrome install <版本>），单测锁形；G3 帽 256MB 收紧 64MB（评审附言）
- 测试：stall 看门狗 mock（64KB 后长睡 450ms 对 150ms 闸）两拍告警字节锚精确加续传全量；既有三测随参数束 ProgressHooks 迁形（clippy too_many_arguments 收束）
- 封版 0.17.0（chrome 下载获能力取 minor）；评审轮与发版随后补记

## 评审一轮（browse-codex-review）

四点名边界全 CONFIRM（看门狗生命周期、sink 泛化错误语义、4MB 闸口径、放宽断言的锁力），无必修加 G1-G4：G2 采纳（chrome 腿收尾行用上 received，死绑定消除）；G3 采纳（推进时刻移 read 后即刷，sink 落盘耗时不算停顿，「等数据」只对网络零字节负责）；G4 半采纳（STALL 常量导出 pub(crate) 复用消字面副本；文案函数维持 pub：稳定文案契约入 aidoc 公开面是收益非负担，doctest 双语境示例已锁）。G1（chrome 腿无读超时，stall 时只告警不退出）记档下批：stall 计数放弃制需独立设计（大包慢速合法，一刀切总超时会误杀）。评审附观察记档：self_update 既有测试固定名临时目录并发互撞（串行绿，属既有卫生面非本批）
