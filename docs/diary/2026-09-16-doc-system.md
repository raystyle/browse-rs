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
