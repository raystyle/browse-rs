---
id: REQ-015
title: 技能触发层与 workspace 单仓（issue #50/#51）
status: draft
priority: should
trace: 待实现后回填（e2e 断言 + 实弹 install/goto 命中）
---

# REQ-015：技能触发层与 workspace 单仓

## Scenario

goto 回执只有 url/title/elapsedMs，agent 每站重学避坑知识；站点知识散在对话、snippets 与前代 bh 仓里无结构、无分发。两个账本 issue 给出同构方案：知识外置独立仓，browse 侧在导航完成点做命中点名，agent 按需拉全文。

## Criteria

- [ ] `browse workspace` 命令族六件：status（安装态/git 态/计数）、install（git clone 种子仓 github.com/raystyle/browse_workspace）、update（pull --ff-only，脏树拒绝）、list、site（段清单或全文）、page（slug 全文）；仓根 `BROWSE_WORKSPACE` 覆盖，缺省 `~/.browse-rs/workspace` 跨实例共享
- [ ] #50 域名层：goto 回执按 URL 域名段（hostname 去 www 首段）命中 `domain-skills/<段>/` 时附 `domain_skills`（文件名列表封顶 10）与 `domain_skills_hint`；未命中回执零新增键（逐字节一致）
- [ ] #51 页面层：goto 后一次有界 pageEval（节点扫描封顶）检出 10 slug 两档置信（spa/hydration/shadow-dom/iframe/iframe-cross-origin/lazy-scroll/bot-shield/login-wall/captcha/service-worker）附 `page_skills`/`page_skills_hint`，框架名单列回执 `framework:{name,version}` 不占 slug；探测失败静默降级绝不失败 goto
- [ ] 两层独立 env 关闭：`BROWSE_DOMAIN_SKILLS=0` / `BROWSE_PAGE_SKILLS=0`（opt-out，镜像 bh 的 BH_DOMAIN_SKILLS=0）
- [ ] daemon env 透传三变量（BROWSE_SECRETS 同款「改配置重启 daemon」口径）
- [ ] e2e 锁：域名命中断言、页面特征命中断言（含置信档）、未命中键集恰为 {elapsedMs,title,url}、分层关闭

## Notes

- 未命中逐字节一致的前提是 serde_json BTreeMap 键序：**禁为此开 preserve_order**（会翻转全部现存回执字节序）
- env 与目录列举逐调用现读不缓存（paths.rs 先例；OnceLock 会把 e2e 的临时 BROWSE_WORKSPACE 注入冻死）
- 种子仓只建骨架（page-skills 10 篇 + domain-skills 样例 + intent-skills 索引）；bh 94 站资产迁移后续批
- 仓内 interaction 机制知识随本批迁 workspace（统一维护，用户令 2026-09-21）：navigation-race/file-download/mouse-input 三篇直迁 intent-skills/，frames-shadow 分流进 page-skills 三篇
