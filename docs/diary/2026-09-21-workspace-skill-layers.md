# 2026-09-21 workspace 技能层批（#50/#51 + 单仓集成）

用户令（2026-09-21）：看最新两 issue（#50 domain 触发层、#51 page 触发层），一次批全做；知识外置单仓 github.com/raystyle/browse_workspace（已建空仓），部署三平台用户目录，git 维护（clone/pull、本地修改可回推）；触发机制裁定为 goto 回执自动点名（issue 原案，「通过 help 触发」即回执 hint 指路）；种子只建骨架，bh 94 站资产迁移后续批；样板参考 browser-harness（bh skill 命令族与 BH_DOMAIN_SKILLS=0 先例）。

## 交付（四 commit）

1. workspace 命令族（31dbedd）：paths::workspace_dir（BROWSE_WORKSPACE 覆盖，缺省 ~/.browse-rs/workspace，刻意不分 BROWSE_NAME：知识是跨实例共享资产，ADR-0008）；workspace.rs git shell-out（install=clone 无 --depth 避 shallow 边角、update=pull --ff-only 前置 porcelain 脏树拒、read_site/read_page 带 canonicalize 越界守卫照 snippets #44 三段式）；main.rs 六 Mode + spawn_blocking（chrome_mgr 同构）；surface 六词条 + env 节三行，手册 117/120 行（六命令表行加示例行，帽余 3）；docs/skills/ 三篇迁 workspace 的 intent-skills/、frames-shadow 分流进 page-skills 三篇，本仓留指路碑；REQ-015 立项。
2. #50 domain 触发层（70d83df）：skills.rs domain 半边（http(s) 门禁 + url_host 去 www 首段 + 排序封顶 10 + hint）；semantic::goto 回执构建后 augment_goto 条件附键（elapsedMs 先固化，探测耗时不算导航时长）；cdp url_host 提 pub；client skills_passthrough_env 三变量透传（Up/run_eval/fetch 三注入点）。
3. #51 page 触发层（ff79047）：PAGE_PROBE_JS 一次 IIFE（TreeWalker 前 800 节点、iframe 判定源 50、无 getComputedStyle、8 秒超时静默降级、恒返合法 JSON 串）；10 slug 固定序两档置信；framework 独立 info 字段；parse_probe/page_fields 纯函数单测锁形。
4. 封版 v0.12.0（本 commit）：能力新增取 minor；ADR-0008 立册；REQ-015 转 implemented；CHANGELOG/diary/architecture/AGENTS/getting-started 随迁。

## browse_workspace 仓诞生（37d4bf8 起两推）

骨架 16 文件：根 README（三级发现面与回推口径）、domain-skills 规约 + github/scraping.md 样例（bh 改编，browse fetch API 优先口径）、page-skills 10 篇配方（4 篇改编 bh interaction-skills 加 6 篇新写，全部 browse 方言语义）、intent-skills 索引加迁入三篇。实测四样本理念沿 #51（x.com=react 容器键、example=static、vuejs.org=vue 版本、vercel=next）。

## 实弹发现（三处真缺陷拦下，一处既有口径确认）

- url_host 对 data: 形返回 scheme 段（"data"）非空串：domain_segment 单测当场拦下，自带 http(s) 门禁 + 段名字符白名单（顺手堵 `http://..../` 的 `..` 段目录穿越面）收口；url_host 文档如实记录边界
- F1 dedup 假失败：技能块新增导航推高 responseReceived 事件水位，peek 取最旧 50 条把 dedup 挤出窗外（计数 0 假红，双通道连红）；窗口 50 扩 200（断言意图是「恰一份」非「前 50 内」），技能块尾补 routeClear 卫生
- page-skills 清单把 README 当 slug：实弹 install 后 list 见 README 顶格，滤除（层索引不是配方）
- BROWSE_CHROME env 面被既有口径拦（非缺陷）：daemon 已在跑时 env 只影响新拉起进程（BROWSE_SECRETS 同语义在册），--chrome 旗标走 /engine/up 请求道直达：实弹排障时按「先查姿势再查配置」确认，issue 不立

## 实弹证据

wsv12 命名实例隔离全链：workspace install 真 clone GitHub（head 对上推送 c6908af）、list/site/page 三读、goto https://github.com/ 回执带 domain_skills:["scraping.md"] 加 hint、framework:react、spa/shadow-dom CONFIRMED、lazy-scroll PLAUSIBLE（真实站点两档置信如设计）、update 干净 ff 与脏树拒（exit 1 加回推 CTA）。e2e 双通道三测全绿（58 秒，含域名命中、特征页六 slug 含置信档、素页键集恰三键、两层独立关闭交叉验证）。[实证: fmt、clippy -D warnings、test --workspace 全绿、doc 干净（预存两条 snippets 文档 HTML 标签警告非本批）、aidoc 31 件 check --strict、surface 投影逐字节、E2E 3 测、PEVO PASS 10]

## 评审闸门与关单（同日续）

- 评审格 browse-codex-review 本批首建（工位 tab 右分 40%，codex YOLO）：三轮闭环。一轮 3F（注释错挂、冻结序分叉、页可控 JSON.stringify 注入面）13G 全处置；二轮快核抓修复自身两缺陷（F3 白名单 doc 新 rustdoc 警告、e2e 注释过度声明）加漂移锁等四 G；终审 CONFIRM 放行
- 推送 2060063..60469ec 七笔（四主体加两评审修加一终审在册），CI 双流绿（ci 35569387902 三岗 1m27s、docs 35569387834 2m53s）
- 评审期实弹新发现并当场拦：url_host data: 形返回 scheme 段（domain_segment 门禁收口）、F1 dedup 最旧窗假失败（窗抬到环形缓冲满额）、page-skills 清单 README 顶格（滤除）
- #52 新开单（timeout 包 session.call 丢 pending 登记，与 semantic.rs 既有同型，cdp 窄接口后续批）；G-F exit 码映射统一与 #52 同账
- 关单：#50/#51 经 omc 集中委托道双关（ledger done，seq 168/169，总台核证五笔祖先与 CI 双绿后收执）；open 集实测 = {52, 1}（1 为常驻冒烟单）

## 后续批候选

- 跨端部署实弹已毕（2026-09-22 补记）：五端 workspace 全 main@02dfb5d（wsl 工作仓、lan-win 早装 update 拉平、lan-mac/lan-ubuntu/lan-linux 全新 install），lan-mac goto medium.com 双层点名实弹全中（domain 2 篇加 page 4 slug 带置信加 framework react）

- bh 94 站 domain-skills 精选迁移（workspace 仓自己的事，#50 注记在册）
- intent-skills 13 篇 bh 配方正文迁移（索引已在册）
- workspace 跨端部署实弹（lan-win/lan-mac/lan-ubuntu 的 ~/.browse-rs/workspace 路径面，CI 三岗盖编译面后按需）
