# Requirements 索引

> 需求登记：新需求先立 REQ 再实现，实现后回填 trace（测试路径或验收命令）。新建拷 0000-template.md，编号接当前最大号。状态 draft 到 implemented 到 rejected。

| id | 状态 | 优先级 | 标题 | trace |
|---|---|---|---|---|
| REQ-001 | implemented | must | 建立文档体系 | code-kit check.py 退出码 0 |
| REQ-002 | implemented | should | browse --llms 发现通道 | browse --llms [--full\|--json] 三形态冒烟 + surface_contract |
| REQ-003 | draft | must | browse 托管 clean-chrome（部署升级维护与自有用户数据） | null |
| REQ-004 | implemented | must | semver 判据与封版流（0.1.0 首封） | tag v0.1.0 + 三路门禁（diary 2026-09-16 封版节） |
| REQ-005 | implemented | should | browse update 自更新面（家族统一标准件） | self_update 单测五件 + 实弹幂等与 ark 拦截 + 评审侧双通道全链 |
| REQ-006 | implemented | must | 批 1 时序口径与导航族（issue #51/#19/#39） | timeout_tests 三件单测加 e2e 批 1 块十断言加 surface 六条目；正文见 REQ-006 文件 |
| REQ-007 | implemented | must | 批 2 token 经济与可观测三件（issue #36/#37/#49） | e2e 批 2 块加真页实弹（example.com）；正文见 REQ-007 文件 |
| REQ-008 | implemented | should | 批 3 原语补全池（issue #35/#38/#40/#41+#24/#42，滚动切片） | 五片全落（e2e 各块与评审轮随卷 diary 批 19） |
| REQ-009 | implemented | should | 抓取与录制批（issue #50/#43） | #50 分类器单测加实弹；#43 v1 实弹（转代码二期记档） |
| REQ-010 | implemented | should | 实例编排批（issue #48/#28） | #48 实弹收口；#28 browse 侧实弹（引擎开关降可选） |
| REQ-011 | implemented | should | 看板与应用生态批（issue #54/#55） | #54 实弹加 XSS/CSP 修复；#55 样例应用实弹三验 |
| REQ-012 | implemented | should | 引擎层扩展域批（issue #27/#61，clean-chrome 联动） | 批 1 至 4 引擎加 browse 侧全落（diary 批 27 至 30） |
| REQ-013 | implemented | must | 账本面（ohmycloud REQ-063 客户端集成与总台修正令收口） | v0.8.0 实弹三绿（6b055cd）加收口批 v0.1.1 GET 实弹与移除面三拒（104f37d） |
| REQ-014 | implemented | should | 引擎附加旗标直通道（issue #48，clean-chrome 扩展域启用面） | 实弹四绿加 spawn_extra_args_shape 锁形；评审两轮 CONFIRM（61d0297） |
| REQ-015 | draft | should | 技能触发层与 workspace 单仓（issue #50/#51） | null（实现中） |

## Roadmap（issue 台账批次规划）

> 台账真源自 v0.8.0 起是 ledger.ohmygh.com（REQ-063，browse 仓 issue/artifact 命令族；旧 issues.ohmygh.com 只读保役，2026-09-20 前的批次记录以旧站为准），本节只排批次与依赖，不复制验收正文（防第二真相）。每批开工立对应 REQ（draft 到 implemented，trace 指 issue id 与验收证据），批完成走 omc 关单。确立 2026-09-19，覆盖当时全部 23 条 open。

| 批 | 条目（issue id） | 主题 | 依赖 |
|---|---|---|---|
| 1 | #51、#19、#39（顺序落地） | 时序口径与导航族：timeout 统一秒加混用告警封顶先行（#19 的 timeout 按秒实现避免分叉），goto 加 clickRef waitNav，历史导航与 check/uncheck/fill submit | 无（#51 必须最先） |
| 2 | #36 + #37 + #49 | token 经济与可观测三件：findRefs 与限深、console/jsErrors/requests、detect 七判（信号源即 #37 缓冲） | 无 |
| 3 | #35、#38、#40、#41（含 #24 残余 annotate）、#42 | 原语补全池，互不依赖可拆小批滚动 | 无 |
| 4 | #45 先行，随 #44 与 #46 | 契约与生态底座：/eval 公开契约文档化，片段库与 skill 分层手册合流 | 批 6 前置 |
| 5 | #48 + #28 | 实例编排与跨仓引擎：按域 cookie 克隆；权限授予与 UA-CH 覆写牵 clean-chrome（飞轮派单） | #28 跨仓协同 |
| 6 | #54 + #55 | 看板与应用插件契约 | 批 4 |
| 长线 | #47、#27 | 穿透语义先验证批证实现状再定语义；引擎扩展域 REQ 加 ADR 设计先行 | 独立于常规批 |

家族其余工位 open 欠账：hst #31（hook 自愈哨兵已落 e7d5581，待关单）；omc/reader/ark/officecli/aria2 零欠账。

