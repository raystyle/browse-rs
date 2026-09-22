# 2026-09-22 #56 批（技能触发事件面扩展）

背景：账本 #56（seq 204，agent 作业中自提的 improvement）：技能触发层只挂 goto 一个事件源，fetch 回执（HTTP 直出与引擎升级两形态）无 domain_skills 键、detect() 七判命中 challenge/captcha/login-wall 时不点对应配方。提案原话：「要人记住该读哪个技能，就已经输了」。

## 修法（口径统一，事件面加挂）

- skills.rs：goto 的域名段匹配抽 pub url_domain_fields(ws, url)（goto 与 fetch 共用口径，fetch 不依赖引擎与 Session）；新增 verdict_page_slugs 判读映射表（challenged 映 bot-shield/captcha、login-wall 映 login-wall，恒 PLAUSIBLE：判读是行为推断非 DOM 实证）与 verdict_fields 入口（BROWSE_PAGE_SKILLS 内检）；page_skills 键对构建抽 page_skills_keys 两事件源共用，回执形同构
- fetch.rs：add_domain_skills 缝，两腿回执都做同口径点名（引擎腿 daemon 侧 goto 的点名回执被抽取串替换，CLI 侧补点）
- js_host.rs detect()：判读构建后接 verdict_fields 插键，未映射判读零新增键

## 守卫与实弹

e2e 补 detect 判读点名（challenged/login-wall）、无映射判读键集恰三键、PAGE_SKILLS 关断还原复测；单测补映射白名单零漂移守卫与 url_domain_fields 形状；fetch 实弹冒烟四象限（HTTP 腿与引擎腿 × 命中附键/未命中零键）。[实证: fmt、clippy -D warnings、test --workspace、test --doc、e2e 真 chrome 3 passed 77s、cargo doc、aidoc --check --strict 31 件、surface_contract 8 passed、PEVO PASS 10]

## 评审一轮（browse-codex-review，本仓首建评审格 w1Q:p2）

四个点名边界全 CONFIRM（augment_goto 重构等价性：serde_json BTreeMap 键序插入序不影响字节；映射无越界无漏；e2e env 窄改还原确定；fetch 双通道零漂移）。F1 必修：版本载体未随批（Cargo.toml 仍 0.14.0，aidoc 版本头将成第二真相；近五批内联 bump 惯例）。采纳：bump 0.15.0 加 CHANGELOG 0.15.0 节并回填 d857861 漏掉的 0.14.0 节。G1-G4 全采纳：diary 本篇、REQ-015 回填 #56（criteria 加一行、trace 与注记随卷）、手写文档面五处同步（getting-started/architecture 两处/README 两处/AGENTS）、fetch 两腿接线单测钉进回归网（TEST_ENV_LOCK 窗内 env 注入，命中附键与未命中键集锁）。

过程一记：首份评审单派发未落地（pane Context 0%、revision 不走，疑 update banner 期吞粘贴），最小回显探针证实通道后重发即正常；--wait 撞上探针已决态即时返回属预期，轮询 agent list 甄别真状态。

## 封版 v0.15.0

能力新增取 minor（REQ-004 判据行）。

## 二轮放行与 G5（同日续）

- 二轮快核（browse-codex-review）：F1 与 G1-G4 全 CONFIRM（评审方独立重跑 aidoc strict clean、browse-cli lib 11 passed、browse-core lib 73 passed、surface_contract 8 passed；3f65a49..037fdb1 的投影 diff 只有版本头行无内容漂移），四边界维持 CONFIRM，放行推 main
- 推送 46defd0..037fdb1 单笔；CI 三岗随后核
- 二轮新增 G5（非阻断，随批收）：ADR-0008 标题/Context 的「goto 回执是唯一天然汇聚处」与 docs/skills/README.md 指路碑仍单事件口径。已收：ADR-0008 冠事件面扩展注记（决策本体不变：条件附加键、零键不变量、单仓外置），指路碑改 goto/fetch/detect（#56），本 docs 提交落在已推 037fdb1 之后不动已核 sha

## 发版 v0.15.0 与五端拉平（同日再续）

- 发版：release.ps1 三目标构建打包冒烟全过，gh release 直发段两遇 GitHub 通道 RST/EOF（uploads 与 api 各一），空 draft（id 393645574）在场；按脚本自家纪律走补挂道：gh release upload 六资产（重试两次过）加 --draft=false --latest 发布，GitHub latest 标记 0.15.0
- 播种：release.yml run 35717887798 success；镜像 stable/latest 回读 0.15.0，stable 段六资产 200。注记：版本段 browse/<版本>/ 公网 404 是既往恒态（v0.13.1/v0.14.0 同），公网面只路由 stable 段，非本批回归
- 五端拉平（全镜像原生链零 token）：wsl、lan-win（回环 ssh）、lan-ubuntu、lan-linux 各 browse update 0.14.0 到 0.15.0，lan-mac ark 委托自升级 0.15.0，五端 --version 实弹
- 新面实弹：wsl 与 lan-ubuntu 各跑 browse fetch medium.com，引擎腿回执 domain_skills 两配方（article-hydration/scraping）加 hint，真仓知识面随包上机
- 拉平中发现姿态缺口提 issue #57：无显示会话（ssh 无 DISPLAY）fetch 引擎腿必挂（spawn 有头 chrome 无 X 即退），BROWSE_ENGINE_ARGS=--headless=new 加 daemon 冷启动可绕行（ensure_daemon_with_env 只透传技能三变量，复用常驻 daemon 不吃新 env）；先 --dry-run 预览后实发入账
