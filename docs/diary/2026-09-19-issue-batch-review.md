# 2026-09-19 issue 整批评审与跨仓对齐

当天事实与裁定：

- issue 台账巡检：15 条（12 open #18 至 #29，加 3 closed），open 全是 09-18 16:10 至 16:30 同批沉淀（首日驾驶循环实测、竞品调研 agent-browser 与 playwright-mcp、源码巡检三流），仓内文档零吸收。
- 用户令整批走 herdr 评审：带起常驻评审格 browse-codex-review（工位 tab 右侧，本仓 cwd），三轮闭环：请求（五件结构）-> 一轮回执 6 CONFIRM / 6 F -> 工位逐锚验证零翻案 -> 修订回执 -> 二轮快核 CONFIRM 终审放行。回执全文落 /tmp/browse-rs-issues-18-29-review.md（评审方临时件，不入册）。
- 一轮 F 载荷主张逐锚验证全中：#18 `\d`/`\s` 静默吞反斜杠（parser.rs 转义 match 的 other 分支）；#19 归因改 Page.enable 时序（wait_for 环形缓冲可取历史、spawn_dialog_watcher 300ms 轮询补开）；#21 auto-parse 证伪，真落点是 member-call bail 加 preview/render_result 字符串裸打（.length 本就是显式特例）；#22 与 ADR-0002 Alternatives 冲突，升 ADR 级；#27 引文 REQ-009/S006/反差分 ADR-0005 归属 clean-chrome（本仓 requirements 实锚 REQ-001 至 REQ-005）；#28 CDP 原生路径在位（methods.txt 的 Browser.grantPermissions:59、setPermission:64、Emulation.setUserAgentOverride:270）；#29 落点改本仓 e2e（pipe-smoke.py 是 Windows-only，WSL 跑不了）。[实证: 评审方回执锚点与本仓源码双锚抽核，零翻案]
- 二轮换锚一处：求值 stdin 的锚是 main.rs 376 至 381 派发与 run_stdin 573（348 是 issue body 的 stdin）。外科建议三条采纳：#25.3 的 --isolated 挪 spawn 线；薄封装批判据改「本仓可闭环、不触跨仓或引擎面」（#24 的 if-changed 与 annotate 是本地像素计算，无 CDP 原语，按旧判据会被误判出批）；#25.2 与 #19 共地基 per-target 钩子（Target.setAutoAttach/setDiscoverTargets 在 methods.txt:622/623 在位未用，可替 300ms 轮询）。
- 终版批次（评审对齐，用户裁定入册后开工）：1 验收补格 #29（patch）；2 渲染与报错带类型 #21(ii)（patch）；3 值模型补面 #21(i)（minor）；4 方言真缺陷 #18 -> #20 -> #19（minor，#20 依赖 3，#19 修时留 per-target 钩子）；5 薄封装批 #23 加 #24 加 #25.2 加 #25.3 的 storageState 往返（minor）；6 生命周期线 #25.1 加 #25.5（proxy 与 self_update 的 HTTPS_PROXY 同口径）加 --isolated；7 凭据线 #25.4 加输出回显脱敏；8 #22 单独 ADR（涉用户裁定，不塞批）；9 跨仓 #28 先 CDP 覆写差分实测（S002）再谈引擎开关、#27 只落近端项（AX depth/分页、CLI 侧 if-changed 与 annotate 对比）；#26 作勘误随 #25 留档，不单独立项。
- 跨仓：clean-chrome#1（GitHub）是能力分界对面载体（判据见该 issue 正文，不在本仓复述；#28 对面已改判 daemon 层，与本仓终审同向）。飞轮纯讨论轮对齐单已插队派 clean-chrome 工位，三件：分界判据确认、pipe 补验交接与 S008 三态矩阵归属、Browse.* 域衔接面约定；回执待收。
- 批 1 开工（#29）：e2e `exercise()` 补 `navigator.webdriver` 恒 false 断言（通道参数化，port 与 pipe 各验一格），README 版本说明注记回填（删「pipe 态未验」，改 e2e 断言在册）。批 diff 走评审格两轮快核：F 二处修（diary 权威句改指针式；CHANGELOG 追正口径收窄为 port 与 pipe 两通道，both 态属 clean-chrome 侧矩阵，本仓 spawn 面本就二选一）加 G 五条采纳；增强格留档：Emulation.setAutomationOverride(true) 下仍断 false（S008 显式开关场景），后续测试增强批再接。[实证: BROWSE_E2E=1 BROWSE_NO_ATTACH=1 cargo test -p browse-core --test e2e 双通道 35.58s 全绿；fmt、clippy、test、doc 四门禁绿；两轮快核 CONFIRM 放行]

裁定：issue 定性的权威载体是 issue 台账与后续各批封版 REQ，本篇只记当轮指针与过程；上文批次表里的 patch/minor 是预报，以封版 REQ 定稿为准。后续各批交付走评审闸门，先闸后推；S008 矩阵 pipe/both 格回填归属候 clean-chrome 工位回执。
