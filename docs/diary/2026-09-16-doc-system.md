# 2026-09-16 文档体系补齐

按 dev-evo v0.6.0 终态清单对齐（ProjectEvo wH 工位派发，dev-evo 工位执行）。当天事实与裁定：

- 补齐 docs 五目录：requirements（README 索引 + 0000 模板 + REQ-001 登记体系本身）、diary（本笔为首笔）、research（暂空，PE-09 SKIP 合法）。
- 新建 docs/README.md 文档地图：活跃体系表加红线注记（diary 与 research 不可裁撤，用户裁定 2026-09-16）；aidoc、surface、architecture.md 以投影与专档身份登记。
- ADR 目录规整以过 PE-06/PE-08：六篇改名 ADR-NNNN-slug.md、头部 bullet 转 YAML frontmatter（0005 的 POSIX 补记移入 note 字段，0006 的关联链接种入正文）、索引补登 ADR-0006、0000 模板同步 frontmatter 化。[实证: 改后 check.py PE-06 与 PE-08 均 PASS]
- 禁字源头清理 29 处：crates 内 `///`/`//!`/`//`/帮助文本 28 处加 getting-started 1 处，破折号改分号/冒号/逗号；aidoc 与 surface 是投影，重生成即随源头变干净。[实证: 复扫 crates 与 docs 手写件无残留]
- surface 技能标题去括号过 PE-10：render_skill 标题只留函数名，完整签名以行内码保留在正文首行；重生成 docs/surface 并过 surface_contract 测试。
- 门禁接入 AGENTS Commands（dev-evo check.py 命令在册）。禁字豁免复盘：手写件源头清零后，aidoc 投影仍有 88 处 em dash，全部是 cargo-aidoc 条目索引的固有分隔符（条目名后接长划线再接说明），工具无开关可改；按 wH 派发预案登记 PEVO_CHECK_ALLOW 路径级豁免 `^docs/aidoc/`，在册于 AGENTS Commands 与 docs/README.md 门禁节，aidoc 真漂移门禁仍是 cargo aidoc --check --strict。[实证: 带豁免跑 check.py，PE-11 报 SKIP 且退出码 0]
- AGENTS Read first 补 docs/README.md 地图入口。
- 途中清了一个残留的 .git/index.lock（08:37 遗留 0 字节，无活跃 git 进程）。[实证: ps 复核后删除，git mv 恢复正常]

裁定：ADR 文件名与头部向 dev-evo 权威模板对齐（frontmatter 置文件顶，id/status 必备）[经验: 迁移不是搬运是重审]；禁字治理分层：手写件改源头，生成物的工具固有渲染格式登记路径级豁免，投影真门禁交给 cargo aidoc --check 与 surface_contract。

## 第六十批：契约注释格式纪律增量

- 首句成句精修 124 处契约注释（browse-cli 13 / cdp 52 / browse-core 59）：首段一句独立成句、不以项名或其动词翻译开头、细节隔空行再写。
- 章节纪律收口出真缺口：browse-core 的 Cargo.toml 没接 `[lints] workspace = true`，missing_docs/missing_errors_doc/missing_panics_doc/missing_safety_doc 整组对该 crate 从未生效；接线并把 errors/panics 两项升 deny。接线后 rustdoc 抓出两处存量断链（parser 文档链到私有 UNSUPPORTED_HINT、surface 模块文档链到不存在的 render），一并修复。[实证: cargo doc --no-deps --workspace 全绿]
- 示例纪律：find_chrome 示例从 println 改为显式路径直通断言；Engine::new 示例从 no_run 升为可跑断言（纯构造无 IO）；五处 no_run（cdp lib/discovery/js_host/server）补注原因（需 tokio runtime 或本机真浏览器）。
- 全平台矩阵首跑（用户指路）：WSL 本机、Windows 宿主机（cargo.exe）、lan-ubuntu、lan-mac 四面 clippy -D warnings 与 cargo test 全绿；mac 验了 POSIX pre_exec 布线面，Windows 验了句柄继承面。[实证: 四面各 12 组 test result: ok，无失败]
- aidoc 投影随注释同批在宿主机侧重生成（24 artifacts），`cargo.exe aidoc --check --strict` clean。

## 第六十一批：终态对齐盘点

- 对照 references 十七篇逐面自评：11 已落、2 部分本批补齐（env-platform 行尾钉死、tool-project 清单与 PEP 723）、2 部分裁定挂起（flow-release 待首次封版、exp-sedimentation 待首个二犯实例激活）、2 不适用（tool-typescript；tool-python 工程面，单件维护脚本由 tool-project 覆盖）。裁定细目见本批回执。
- env-platform 补：新增 .gitattributes 钉行尾 lf；renormalize 仅 methods.txt 行尾归一，内容零变化 [实证: ignore-cr-at-eol 空 diff]。CI 三系统矩阵裁定待远端（仓无 remote，无处落）。
- tool-project 补：tools/ 增 README 清单；gen-cdp-methods.py 加 PEP 723 头（零依赖声明）。
- agent CLI 面裁定（tool-cli-agents 第十一节对照）：--llms 发现通道是真差距，立项 REQ-002（draft）；MCP 通道、脚本 workspace、cl100k 计量分页、类型化错误码信封裁定不适用或等价形态已落：方言值优先输出加大值落盘指针行（`{__dropped}`）即 token 经济学形态，分页由读文件侧承担，错误带可照抄下一步 CTA 比错误码更强；skill 已按命令组拆分（全局函数面），命令面单一真相源三面派生已落（surface_contract 锁漂移）。
- 排查退役三篇（env-environment / flow-events / tool-selection）引用：全仓零命中，无需清理 [实证: rg 全仓扫描空]。
- 用户确认四端全平台测试验收成局（lan-win 总台、lan-ubuntu、lan-linux、lan-mac）；本批改动全为非代码面，无平台方差，不另加跑。
- 总台周知（ohmycloud，用户宣言 2026-09-16）：四平台协议升为全仓周知面，验收按需向 ohmycloud 总台要端点测试支撑，Rust 三岗 CI（linux 加 win-gnu 交叉加 mac）支撑已落地；本仓协作面（AGENTS 环境节）同步补入，远端建立后挂三岗 CI（衔接 flow-release 的 CI 待远端裁定）。
- 统一封版协调（总台令）：治理终态句已回报（合同在位、门禁四端绿、CI 待远端、dev-evo 形态齐），版本判定「封 0.1.0 待封」（零 tag 无 CHANGELOG，现存全部功能为 0.1.0 主体），候总台统一封版令。
- 第六十三批跟进：AGENTS 环境节补连接姿势口径引用一句（WSL 到宿主恒走回环与 interop、不走宿主 mesh IP；lan 三端 mesh 随时随地；口径在 env-platform 第十节）；周知多仓飞轮协作协议成篇（flow-flywheel.md），本仓派单回执实践即其实证源。
