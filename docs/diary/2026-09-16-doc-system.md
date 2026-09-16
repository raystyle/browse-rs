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
- 第六十四批跟进三项：环境节补 pwsh 统一载体句（既有 PEP 723 Python 载体不迁移，新增验收/运维脚本一律 pwsh）；Must 节补版本载体唯一权威句（Cargo.toml workspace 版，semver 判据进封版 REQ；扫描确认载体外无第二真相，ADR 冻结件散文不算）；分发体系裁定待远端首发时接入（seed 通道自推镜像 + 三平台 CLI 资产，omc 管资源分发、ark 管落地验收，当前无分发面不预建）。版本判定维持「封 0.1.0 待封」。
- 用户新令落账：browse 集成 clean-chrome 成独立软件包，自有应用数据目录，负责各系统部署升级维护。立 REQ-003（draft，must）：部署/升级/doctor 三面、自有 user-data 绑 `<state>/`、托管位并入发现序、分发走镜像域优先 GitHub 回退（对齐 flow-release 第八节）；定标项显式在册（镜像命名、pin 载体、跨平台资产、目录命名），实现批先立 ADR-0007；按 semver 判据落 0.2.0 批，不阻塞 0.1.0 统一封版。
- REQ-003 定标令（用户裁定 2026-09-16）：omc 负责 clean-chrome 发布包资源分发；browse 自管安装执行，复制 clean-chrome 既有各版本安装部署升级形态（不自创）；ark 不管 clean-chrome 安装（fleet 执行角色的本仓例外）。已折进 REQ-003 正文角色裁定段，剩余定标项五个（镜像 tool 段命名与 omc 协调、pin 载体、跨平台资产、部署目录命名、发现序优先级）。

## REQ-003 实现批：本地导入面全链

- 新模块 `browse-core/src/chrome_mgr.rs`：版本登记册（manifest.json：已装版本/来源/文件与字节基线/pin）、本地目录导入安装（整树复制 + chrome 二进制校验 + 自动 pin）、pin 切换（旧版保留可回退）、doctor（在位/文件基线/pin 健康，漂移时托管位自动退发现序）、版本号防穿越校验；五组单元测试全绿。
- 发现序落地（engine `resolve_chrome`）：`--chrome` 显式 -> `BROWSE_CHROME` -> 托管 pin（`<state>/chromium/<pinned>/`）-> 祖先部署 -> 常规路径；spawn 找不到 chrome 的错误 CTA 改指 `browse chrome install`。
- 命令面 8 条进 `COMMANDS`（Cli：browse chrome install/list/use/doctor；全局：chromeInstall/chromeList/chromeUse/chromeDoctor），方言侧 install 走 spawn_blocking 防塞单飞槽；未知函数 CTA 清单同步。
- Windows 真机冒烟 [实证: 真部署目录导入 500 文件/683MB，list/use/doctor 输出健康]；四面门禁绿（WSL、Windows 宿主、lan-ubuntu、lan-mac 各 12 组 ok）；surface 与 aidoc（25 artifacts，+chrome_mgr 模块页）重生成。
- clean-chrome 工位转话（c002325）三句裁定与本仓 ADR-0007/REQ-003 同向，已确认入 REQ-003 上下文；R2 下载腿保持待 omc 端点定标，REQ-003 维持 draft（判据未全过不回填 trace）。
- 分发承载定令（用户令 2026-09-16，clean-chrome 工位转达）：clean-chrome 发布包资源分发在 ohmygh.COM 域下专门子域名承载，omc 承建；本仓勿自建分发腿（GitHub 直连腿候选资格同废），子域名与路由候 omc 总台定标回执后接入。已折进 REQ-003 角色裁定段与 ADR-0007 决策二（Alternatives 同步钉死）。
- 子域名定标回执（omc 总台，clean-chrome 工位转话）：chrome.ohmygh.com（与 env/pkgs/registry 平级），R2 桶 chrome 已建，版本段路由 <version>/<asset> 加 .sha256 边车（与 env 域同构，无 manifest 边车即锚）；DNS CNAME 至 public.r2.dev 传播中，总台热验回报后本仓再接 R2 下载腿实测。端点已入册 REQ-003 与 ADR-0007 决策二；定标余量收敛为版本发现来源与资产命名（随总台首版资产确认）。
- 远端同步（用户令）：github.com/raystyle/browse-rs 建仓（public、空仓），origin 接入并推 main（e3bd2d0）；按既挂裁定（远端建立后挂 Rust 三岗 CI）补 .github/workflows/ci.yml（linux 本职 + win-gnu 交叉 + mac 本职，工具链钉版驱动），AGENTS 环境节同步改「已挂」。CI 待首跑回报。
- 架构定调周知（用户裁 2026-09-16）：立 ADR-0007（accepted）：browse 内嵌 Chromium 版本管理器不依赖外部安装，各版本落 `<state>/chromium/<version>/` 版本化管理，机制复制 clean-chrome 既有形态；omc 管发布包资源分发（R2 镜像 + 版本段 + 边车锚，同 ark/hst 链）；ark 明确排除。REQ-003 同步对齐（manifest 版本登记、原子落位、pin 切换入 Criteria），定标项收敛为四个（镜像 tool 段与版本清单端点、缺版本自动装或 CTA、跨平台资产、GitHub 回退腿）。

## 开发仓位迁移（总台周知）

- 本机 WSL 十仓迁入独立 VHDX（D:\wsl_workspace\repos.vhdx，50GB ext4，挂载 /mnt/wsl/repos，快捷 ~/repos），browse-rs 在列；登录自动挂载已注册（WSL-RepoDisk schtask）。
- 后续新开发会话与日记以 ~/repos/browse-rs 为工作根（今日会话起于旧位 /mnt/c/browse-rs，收尾后新 clone 拉平到 tip）；旧位保留过渡一至两天后裁。
- 本仓跨仓路径切形：AGENTS Commands 与 docs/README.md 门禁节的 dev-evo check.py 路径由 /mnt/d/ProjectEvo 切 ~/repos/ProjectEvo（新位实证同版可用）；矩阵 rsync 源同步切 ~/repos 形。
- Windows 侧注意：aidoc 投影与 cargo.exe 同树面在过渡期仍走旧位 /mnt/c 树，新根的 Windows 岗衔接随旧位裁撤批再定。

## 平台原语定标（用户令 2026-09-16）

- 全平台测试基建正确原语定标为 5端4机：五端 wsl、lan-win、lan-mac、lan-ubuntu、lan-linux，wsl 与 lan-win 同宿主合计四台物理机；linux2 系 lan-linux 误名，不是端点名（错名源头在 ohmycloud AGENTS pwsh 五端清单，已 herdr 转达总台自清，本仓 grep 复扫零引用）。
- AGENTS 环境节由「四平台测试矩阵」形改「5端4机」形：wsl 独立成端不再折进 lan-win 括号，「四平台」此后仅指 OS 维度（win/ubuntu/linux/mac）。[实证: 改后门禁 PE-01 至 PE-12 exit 0（PASS 10 / SKIP 2），载体用重构后新位 evo-adr:code-kit 的 check.py]
- 途中发现：ProjectEvo 仓本日更名 project-evo 且重构为四插件市场仓（evo-adr / evo-codesec / evo-research / evo-herdr），dev-evo skill 消解；本仓 AGENTS Commands 与 docs/README.md 门禁节所引 ~/repos/ProjectEvo/.../dev-evo/scripts/check.py 旧路径悬空，候总台新口径周知后另批切径，本批不夹带。

## project-evo 切径收口（总台回执 ff8255a）

- 总台回执三件全过：A 活跃面正名（AGENTS:39 linux2 改 lan-linux 并注五端四机）、B REQ-049 与索引追记端位承接、C docs/README 切四插件形态；豁免裁定九文件全对号（历史档按当时事实保留），门禁双绿，sha 已自取核对。
- 本仓收口五处切 evo-adr:code-kit 新位：AGENTS Commands 门禁路径、AGENTS 连接姿势口径引用（env-platform 第十节，节号随迁未变）、docs/README 门禁节路径、requirements 索引 REQ-001 trace 锚、REQ-002 出处；REQ-001 文内 trace 与 diary 旧路径叙述按当时事实保留（同总台豁免口径）。[实证: AGENTS 所载新命令逐字直跑 exit 0（PASS 10 / SKIP 2）]
