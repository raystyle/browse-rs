---
id: ADR-0008
status: accepted
date: 2026-09-21
deciders: 用户（触发机制与单仓裁定）、raystyle
superseded_by: null
---

# ADR-0008：goto 回执技能触发层与 workspace 单仓外置知识

## Context

账本 #50/#51（2026-09-21）要给 agent 三级发现面：goto 回执自动点名 -> hint 读全文命令 -> 知识仓全文。知识资产（94 站 domain-skills、17 篇 interaction-skills）在前代 browser-harness 里，更新节奏与工具二进制解耦。daemon 无按名路由（方言统一 POST /eval），goto 回执是导航完成点唯一天然汇聚处。用户裁定：单仓 github.com/raystyle/browse_workspace、三平台用户目录部署、git 维护、goto 回执自动点名（issue 原案）。

## Decision

技能触发走「goto 回执条件附加键」而非新命令或新事件：命中 workspace 知识时回执附 `domain_skills`/`page_skills` 清单与 hint 键，未命中或关闭时一个键都不加。未命中零新增键是回执契约的硬不变量（serde_json BTreeMap 键序下与现状逐字节一致，禁为此开 preserve_order）。知识仓外置独立仓，git shell-out `git`（不引 git crate）做 install（clone）/update（pull --ff-only），本地修改可手工 commit/push 回推；仓根约定 `~/.browse-rs/workspace`（BROWSE_WORKSPACE 覆盖），刻意不分 BROWSE_NAME 命名空间（站点知识是跨实例共享资产，与 daemon 端口、引擎 profile 的按名隔离方向相反）。env 与目录列举逐调用现读不缓存（paths.rs 先例；缓存目录会挡住回推后即时可见，OnceLock 会冻死 e2e 注入）。

## Consequences

- 好：agent 按需拉全文不占上下文；知识更新零工具发版；未命中行为零变化；两层（domain/page）各自 env 开关独立降级
- 坏：goto 每次多一次有界 pageEval（约几十毫秒，8 秒超时兜底，`BROWSE_PAGE_SKILLS=0` 关）；回执契约从恒三键变条件键集，下游严格 schema 消费方要按可选键建模；git 成为 install/update 的外部依赖（缺失给 CTA 不内嵌降级实现）；`BROWSE_NAME=workspace` 实例状态目录与仓根同路径（状态文件名不撞 domain-skills/page-skills，共存可接受）；域名首段规则对 co.uk 类多标签 TLD 不特判（bbc.co.uk 的段是 bbc，够用口径）；`browse fetch` 引擎腿内部走 goto 也吃一次探测（回执键被丢弃，仅时延，两开关可关）；探测串由页内 JSON.stringify 产出、页面可覆写伪造，回执侧以冻结名单白名单校验收口（评审 F3）

## Alternatives

- 新命令显式查询（browse workspace suggest）：被否：错过导航完成点的免费汇聚，agent 要多一步主动查询；保留 list/site/page 作为手动下钻面而非触发面
- 知识内嵌 browse 仓 docs/skills：被否：知识更新与工具发版耦合，94 站资产体量进代码仓污染 diff（用户裁定统一维护外置）
- 引 git2/libgit2 crate：被否：重依赖换两条 clone/pull 命令不值得，shell-out 缺失时 CTA 指手工 git
- OnceLock 进程级缓存 env/目录：被否：见 Decision 末段，三输（e2e 死、回推不可见、收益纳秒级）
