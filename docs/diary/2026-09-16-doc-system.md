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

## REQ-002 实现批：--llms 发现通道

- CLI 接线三形态：`--llms`（紧凑清单）/ `--llms --full`（完整版）/ `--llms --json`（Schema 包，复用既有全局 `--json` 旗标），解析后先于 serve 与求值直出 stdout 后返回，不触 ensure_daemon；优先级 json > full；`--help` 补用法两行。
- 目录登记 `llms-flag` 进 `COMMANDS`（CLI 节，status 之后），docs/surface 四件随之重生成（schema +20 / llms-full +13 / llms +1 行，SKILL.md 不动因其只渲染全局函数）。
- 冒烟三形态：输出与 docs/surface/llms.txt、llms-full.txt 逐字节同源（diff 空），--json 合法 JSON 且 definitions 59 含 cli.llms-flag；全程 9880 零监听（BROWSE_PORT=9979 隔离跑），daemon 未拉起。[实证: diff 空输出、ss 复核前后计数 0]
- 门禁：clippy -D warnings 绿、cargo test --workspace 12 组 ok、doc test 10 ok、surface_contract 3 ok（先红后绿：目录改动后投影重生成即过，锁漂移机制实证）。
- aidoc 不重生成：本批零 pub 项与 /// 改动（COMMANDS 是常量数据，main.rs 全私有项），投影构造上无漂移；WSL 侧 cargo aidoc --check 平台门控 NOT CHECKED（msvc 钉死属工具明示行为非失败）。
- 版本判定：REQ-002 属 0.1.0 主体增量（封版前落地），semver 裁量随封版 REQ 统一，Cargo.toml 不动。

## skill 物种退役与 aidoc 视图定标（用户令 2026-09-16）

- 用户裁定：有 `browse --llms` 即不需 skill 形态命令，--llms 就是给 agent 的紧凑说明书。落地：`render_skill` 删除（pub 项），write_surface_files 不再产 SKILL.md，docs/surface/skills/ 删除，契约测试改锁「skills/ 不得残留」，AGENTS 与维护注释同步；llms.txt 尾注补「二进制直出 browse --llms（--full / --json 同源）」。
- aidoc 投影随 pub 项删除必重生成，途中澄清口径：**本仓 Windows 编译面是 win-gnu 非 msvc**（用户裁定，CI ci.yml win-gnu check 岗即此形态），旧「宿主机 cargo.exe 同树重生成」句作废（旧位 /mnt/c 树已裁、UNC 共享对 interop 不可达）。落地：wsl 端 `cargo aidoc --retarget` 迁主机视图（仓内零 target_env cfg，pipe.md 页面 Windows/POSIX 变体随视图对换，语义等价），25 artifacts 重写，check --strict clean；nightly-2026-07-07 工具链补 target std 后一次过。[实证: check clean exit 0，diff 6 文件 +25/-28]
- CI docs 岗随迁：windows-latest（msvc 宿主）改 ubuntu-latest，checkout 顺升 v5；Windows 面门禁由 ci.yml win-gnu check 岗独担。代价入账：Windows 宿主上跑 cargo test 的运行时覆盖从 CI 退役（按「不编译 msvc」裁定让位，后续如需 mingw 实跑岗另立）。
- 门禁：clippy 绿、12 组 test、10 doc test、surface_contract 3 ok、aidoc check clean、PE-01 至 PE-12 exit 0。

## 封版 v0.1.0（用户令 2026-09-16）

- 三路门禁（flow-release 第二节）：wsl 全件（fmt --check、clippy -D warnings、12 组 test、10 doc test、release 构建 1m09s）；实机 lan-ubuntu 与 lan-mac（rsync 同树，STAGE-CLIPPY-OK 加 STAGE-TEST-OK，各 12 组 test ok，exit 0；ubuntu 首跑 PATH 127 补 source cargo/env 即过）；CI main 双岗绿（5fe8804：ci 1m13s、docs 2m17s）。lan-win=win-gnu 交叉岗、lan-linux=备用端点，在册口径不适用实跑。[实证: 各路阶段标记与退出码直读]
- 封版件一次提交：CHANGELOG 立卷（版本级里程碑制）、REQ-004 立 semver 判据（首封 0.1.0：零前置版不取 1.0.0；R2 腿后续取 0.2.0）；生成物零版本号嵌入（surface 与 aidoc manifest 扫描空），无重生件。
- tag v0.1.0 推远端触发 CI；分发腿不预建（用户裁定在册，CLI 资产归 omc/seed 通道），仓内无 release 流水线属预期非缺口。
- 收尾义务：CHANGELOG 定版、本日记钩子、总台版本对齐表 browse-rs 行回执（tag 落地后 herdr 转发）。

## REQ-003 R2 下载腿实现批（总台热验回执后开工，2026-09-17）

- 总台正式热验回执到：chrome.ohmygh.com 验讫（HTTP/HTTPS/H2 通、HKG 边缘在位、TLS 链净、桶在役无对象态），分发基建就绪。本仓即开工 0.2.0 批。
- 实现面：chrome_mgr 增 `install_from_mirror`（env 覆写包装 `BROWSE_CHROME_MIRROR`/`BROWSE_CHROME_ASSET`）与内核 `install_from_mirror_with`（显式镜像与资产名，测试与程序化面）；边车锚先行（锚不在不拉大包）、流式下载同窗 sha256、zip 解包单顶层目录下钻、同盘 rename 原子落位、staging 用后即清；CLI `browse chrome install <版本>` 部署目录缺省走镜像，方言 `chromeInstall({version})` 两形分流；资产名暂定约定 `chromium-<version>.zip` 候首版资产定标。
- 坑与修：2024 版 env::set_var 属 unsafe，测试改直调内核参数束零 env 动作（并行安全顺带解决）；真冒烟抓到单测盖不住的坑：reqwest blocking client 在 async 上下文 drop 会 panic，CLI 直调面收 spawn_blocking 与方言侧同式（门禁全绿下的漏网实证，flow-release 门禁与实跑互补纪律的注脚）。
- 实测：mock 镜像三态全绿（happy 落位登记 pin 零残件、锚不匹配错包即弃、404 CTA）；真端点负测 `chrome install 9.9.9.9` 得 404 错误带端点与覆写指引 exit 1；happy-path 真资产实测随 clean-chrome 首版资产落桶（REQ-003 trace 届时回填）。
- --version 旗标并入本批（原报总台的 0.1.1 patch 并入 0.2.0，少一次 tag）；workspace 版本 0.1.0 升 0.2.0，repository 元数据正本 clean-chrome 改 browse-rs；surface 与 aidoc（25 artifacts）重生成；新依赖 zip/sha2 入册，reqwest blocking 特性挂 browse-core。
- 门禁：clippy -D warnings 绿、12 组 test、10 doc test、surface_contract、aidoc check clean。

## clean-chrome 编译窗回执吸收与验收分工（2026-09-17）

- 编译窗提前令回执（clean-chrome 工位，cd632e3 已推）五件全过：152.0.7977.84 三机全清重编（win/mac/ubuntu 的 out\Dev 加 out\Release 齐绿，lan-linux 不适用在册口径）；R001 与根 args.gn 唯一权威，偏差一笔 M028（junction 乘 siso 不兼容，Windows 走物理路径出产物，ADR-0008 修订已上浮用户）；补丁双形态 34 文件 byte-identical；pipe-smoke 过；check.py exit 0。
- 自证（对方陈述不作数）：cd632e3 在册且提交语含验收分工句；diary 2026-09-17 八处实证；本机探 Windows 恒定逻辑路径下 Release 与 Dev chrome.exe 双双在位（/mnt/c/dev-chrome/chromium/src/out/，Release 含部署面件）。[实证: ls 直读]
- 用户令落账：chrome 产物验收测试自下窗起归本工位，clean-chrome 构建自证止于产物在位与自检冒烟；已入 AGENTS 环境节。155 编译窗的 browse 侧验收面（导入安装、驱动冒烟、跨端实测）届时随窗执行；R2 首版资产（zip 加边车）落桶后 REQ-003 happy-path 实测与 trace 回填同窗衔接。
- 导入源衔接在册：Windows C:\dev-chrome\chromium\src\out\Release（恒定逻辑路径经 junction 可达）。

## lan-linux 激活为无头验证端与平台测试分工（用户令 2026-09-17）

- 用户令三句定型测试分工：wsl 与 lan-linux 走无头测试（lan-linux cp linux 产物专职无头验证），lan-win、lan-mac、lan-ubuntu 带桌面走有头与附着测试；已入 AGENTS 环境节 5端4机 行。
- 产物管道：lan-ubuntu out/Release 精选运行时集合 867MB（chrome 661M 加 locales 125M 加 paks/icudtl/snapshot 双件加 libEGL 等七 so 加 IwaKeyDistribution/MEIPreload/PrivacySandbox/angledata 四目录）；ubuntu 至 lan-linux mesh 直连不通（ssh 255），tar 管道走 wsl 双跳中转；deploy-release.py 白名单是 Windows 形（chrome.exe/dll），linux 运行时集合系本批实证选取，候选 155 窗回填 clean-chrome 侧工具化。
- lan-linux 全链五阶绿：chrome --version 直通（Chromium 152.0.7977.84，ldd 零缺库，服务器库面全）；browse 0.2.0 二进制直发（wsl glibc 2.39 产物跑 lan-linux 2.43，前向兼容实证）；chrome install 导入 480 文件/907MB 自动 pin；无头链 up --headless（spawn headless=true channel=port）加 navigate data: URL 加 evaluate 得值 lanlinux-headless-ok 加 doctor healthy/pinnedOk 双真加 down 干净退。[实证: 阶段标记 STAGE-UP/EVAL/DOCTOR/DOWN-OK 直读]
- wsl 无头面同套激活：同集合导入后首跑缺 libnss3/libnspr4/libasound2t64 五库，apt 装后 ldd 零缺；无头链全绿（evaluate 得值 wsl-headless-ok，doctor 双真）。wsl 依赖面入账：最小 Ubuntu 缺 nss/alsa 族，lan-linux 服务器库面反而全。
- REQ-003 跨平台注更新：Linux 运行时集实证可跑（两端无头全链），资产打包形候 155 窗。

## x.com 登录态 cookie 转移实测（用户令 2026-09-17，lan-win 有头面首验）

- 链路：lan-win 起 152 有头窗（持久 profile C:\Users\ray\.browse-rs\x-login-profile，9222 调试口）加用户手工登 x.com 加 wsl 侧 browse 经 mirrored 回环 127.0.0.1:9222 附着读取（x.com 与 twitter.com 双域 19 枚含 auth_token）加 BROWSE_NO_ATTACH=1 spawn 无头引擎（engine-profile）加 Network.setCookies 整批注入加无头开 x.com/home 验证。[实证: 注入回 injected-ok；无头引擎 location.pathname 得 /home（未认证必 302 至 /login，协议级登录态证据）]
- 跨端附着通路实证：WSL 到宿主 127.0.0.1:9222 直达（mirrored 回环，与连接姿势口径一致）；守卫拦 Browser.close 属设计内（附着浏览器绝不被工具关，实测命中一次）。
- 真缺口一（bug，候 patch 批）：附着 tab 因页面导航换血断 socket 后，browse up --connect 9222 重附只重建 Engine 层（status 显示 attached），JsHost 内层 Session 仍持死 socket，一切求值 CDP socket closed，须 browse down 重起 daemon 才恢复；修法候选：engine 重附时同步重建 host 会话。
- 真缺口二（行为澄清）：引擎已健康附着时 up --headless 不切换（attach-first 教义使然）；强制换 spawn 要 BROWSE_NO_ATTACH=1 且 daemon 进程须带该 env 起动（旧 daemon 不读新 env），实战记 down 后再带 env up。
- 方言边界三处实踏（皆在册设计）：无箭头函数（过滤移 jq/shell）、无加号运算符（写字面量）、成员访问限 .prop 形（页面逻辑照旧走 Runtime.evaluate 的 expression）。

## attach 断线不愈修复批（0.2.1，2026-09-17）

- 病灶：Session 存活旗 connected 在 open_ws 与 connect_pipes 两处都被读循环写进**局部** Arc<AtomicBool>（新建对象），self.connected 永远停在 true；is_connected 谎报活着，Engine::ensure 开头短路不重连，server 求值前懒 ensure 同样不触发，附着 target 换血断线后一切求值 CDP socket closed 直到重启 daemon（x.com cookie 转移实测中首见）。
- 修法：connected 字段转 Arc<AtomicBool>，两处读循环持克隆，WS EOF 与管道 EOF 即翻 false；零 pub 面变化（字段私有），aidoc 零漂移。
- 回归测试 connected_flag_falls_when_ws_dies：本地 ws 服务端握手即断，断言旗必翻 false（旧实现此断言永败）。
- 自愈实证（真场景复现）：spawn 无头引擎 pid 1290712 后 kill -9，下一次求值直接返回 healed-ok 并自动重起引擎 pid 1290949，status 健康；旧行为是永久 socket closed。
- 观察入账：懒 ensure 重起走缺省 Auto 形态（headless=false，wslg 有显示面所以有头也能跑），显式 headless 要再 up --headless；后续如需「记住上次显式形态」另立行为批裁量。
- 门禁：clippy 绿、12 组 test、10 doc test、aidoc check clean、PE-01 至 PE-12 exit 0；封 0.2.1（patch 判据：修复批）。

## 自定义引擎 profile 批（0.3.0，用户令 2026-09-17）

- 用户令：默认用固定 profile 持久保存站点状态与会话（既有 engine-profile 语义即此，down 不删、登录态复用，x.com cookie 注入即落此），并补自定义 profile 能力。
- 实现：EngineSpec::Auto 增 profile 字段（pub 面，aidoc 25 artifacts 重生）；spawn 与 spawn_pipes 两通道同接（profile 缺省解到固定 engine-profile）；BROWSE_PROFILE 环境缺省由 from_env 读（daemon 懒 ensure 重起也吃），CLI --profile 显式顶掉；HTTP /engine/up 的 EngineUpRequest 增 profile；目录 up 条目签名与参数同步。
- 冒烟 [实证: BROWSE_NO_ATTACH=1 up --headless --profile /tmp/custom-profile-a 得 spawned profile /tmp/custom-profile-a，目录落位，status 报同值；默认 profile 语义不变]。
- 门禁：clippy 绿（run_eval 旗标族透传 allow too_many_arguments 在册注因）、12 组 test、10 doc test、surface 与 aidoc 重生、check clean、PE-01 至 PE-12 exit 0。封 0.3.0（minor 判据：能力新增）。
- 踩坑注：down 后立即 up 偶发旧 daemon 未及退净致附着旧 9222，重跑即过；非本批引入，未立票。
