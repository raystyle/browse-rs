# browse_rs

Rust workspace（crates/cdp、browse-core、browse-cli）。公开契约以 `///` 与类型签名为准；toolchain 见 rust-toolchain.toml（1.98.0）。仓库根的 `chromium-*/` 是 clean-chrome 部署产物，已 .gitignore，勿动。

## Commands

- cargo fmt --all
- cargo clippy --workspace --all-targets -- -D warnings
- cargo test --workspace
- cargo test --doc --workspace
- cargo doc --no-deps --workspace
- cargo aidoc --check --strict   # aidoc 投影漂移门禁（改动 pub/文档后先 cargo aidoc 再提交 docs/aidoc）
- cargo run -p browse-cli -- --gen-surface docs/surface   # 命令面目录重生成（schema/llms；tests/surface_contract.rs 锁漂移；agent 发现通道 browse --llms 同源直出）
- cargo install --path crates/browse-cli --force    # 本机装 browse 进 PATH
- BROWSE_E2E=1 BROWSE_NO_ATTACH=1 cargo test -p browse-core --test e2e # 真 chrome 端到端（NO_ATTACH：本机 9222 开着用户浏览器时也要自起隔离实例；CI 跳过）
- PEVO_CHECK_ALLOW="^docs/aidoc/" uv run ~/repos/project-evo/plugins/evo-adr/skills/code-kit/scripts/check.py .   # 文档骨架合规门禁（PE-01 至 PE-12，退出码 0；载体 project-evo 四插件市场仓 evo-adr:code-kit；豁免正则在册：aidoc 条目分隔符 em dash 是 cargo-aidoc 渲染格式，无开关，真门禁是 cargo aidoc --check --strict）

## Must

- 改 pub 项：同步 `///` 与 doctest（missing_docs 是 deny，CI 必红），并 `cargo aidoc` 后提交 `docs/aidoc/`
- 契约注释三纪律（dev-evo 第六十批）：首句成句（做什么+何时用+边界，不以项名开头，细节隔空行）；返 `Result` 必备 `# Errors`、可能 panic 必备 `# Panics`（clippy missing_errors_doc/missing_panics_doc/missing_safety_doc 已 deny）；示例断言收尾，`no_run` 注明原因
- 版本载体唯一权威：Cargo.toml 的 workspace 版本；载体外出现版本号即第二真相，清理（ADR 冻结件里的决策语境散文不算）。semver 触发判据：文档/修复批取 patch，能力新增或行为变化取 minor，契约破裂或形态重构取 major；判据写在封版 REQ 里
- 不可逆技术选择：先写 docs/adr/ 或改旧 ADR 的 Status
- 文档链接只用 intra-doc（`` [`Session::call`] ``）
- I/O 类示例标 no_run，不标 ignore
- spawn 引擎保持 `--no-sandbox`（SxS 部署沙箱打不开自身 exe，见 ADR-0003）
- 遇缺陷当场一键反馈：`browse issue new <标题> --acceptance <验收> --body <正文>`（账本真源 ledger.ohmygh.com，REQ-063 契约；写入走 Ed25519 五头签名道，先 `browse ledger keygen` 且公钥 kid 总台在册；旧 issues.ohmygh.com 只读保役；agent 作业中发现 browse 自身缺陷先提 issue 再绕行，契约实弹先 `--dry-run` 预览零入账）

## Must not

- 手改 docs/aidoc/（生成物，下次 cargo aidoc 会覆盖）或另写 API.md 当第二真相
- 把 ADR 正文贴进函数文档；不整份读 llms-full.txt
- 关用户浏览器：守卫在 cdp `Session::call` 层，绕不过是特性不是 bug
- E2E 测试共享 engine-profile（chrome 单实例锁；tests 里用 SEQ 互斥串行）

## Read first

- docs/README.md（文档地图：五目录与投影专档索引；diary 与 research 不可裁撤，用户裁定 2026-09-16）
- docs/aidoc/llms.txt（Agent 入口索引）-> 相关 docs/aidoc/<crate>/<module>.md
- docs/surface/llms.txt（CLI/方言命令面清单，crates/browse-core/src/surface.rs 派生）
- docs/architecture.md（现在怎么拼：三层 crate + daemon + 双通道）
- 仍不确定再打开源码 `///`
- docs/adr/ 仅在改对应决策时（0001 daemon / 0002 方言 / 0003 引擎策略 / 0004 自动附着 / 0005 管道）
- docs/guides/getting-started.md（CLI 使用面）

## 环境

- daemon 端口默认 9880（BROWSE_PORT）；BROWSE_NAME 命名实例（状态目录 + 派生端口 9900-9999，ADR-0006）；chrome 发现序：BROWSE_CHROME -> cwd/exe 祖先的 `chromium-*/chrome.exe` -> 常规路径；BROWSE_NO_ATTACH=1 跳过附着探测强制 spawn
- workspace 技能仓（ADR-0008，#50/#51 配套）：知识外置单仓 github.com/raystyle/browse_workspace，部署 `~/.browse-rs/workspace`（BROWSE_WORKSPACE 覆盖，跨实例共享不分 BROWSE_NAME）；`browse workspace status/install/update/list/site/page` 六件（git shell-out，本地修改可 push 回推）；goto 回执按域名段与页面特征自动点名（BROWSE_DOMAIN_SKILLS=0 / BROWSE_PAGE_SKILLS=0 分层关，env 透传给新 daemon，改配置重启 daemon）；机制配方与站点知识的增改在 workspace 仓做，不回本仓 docs/skills/（已留指路碑）
- 域策略：BROWSE_DENY_DOMAINS / BROWSE_ALLOW_DOMAINS（后缀匹配，deny 优先），拦 Page.navigate 与 Target.createTarget
- 652 命令清单 crates/cdp/src/methods.txt 是生成物（tools/gen-cdp-methods.py，源头 refs/ 不入库）；改协议版本重跑生成再提交
- daemon 日志：`%USERPROFILE%\.browse-rs\daemon.log`；引擎 chrome 诊断：同目录 `engine.log`（不继承调用方句柄）
- 本机引擎 profile：`%USERPROFILE%\.browse-rs\engine-profile`（down 不删，复用登录态；`--profile <dir>` / `BROWSE_PROFILE` 换自定义 user-data-dir，用户令 2026-09-17）
- 全平台测试基建（5端4机，用户定标 2026-09-16）：五端 wsl、lan-win、lan-mac、lan-ubuntu、lan-linux，wsl 与 lan-win 同宿主合计四台物理机；测试分工（用户令 2026-09-17）：wsl 与 lan-linux 走无头测试（lan-linux cp linux 产物专职无头验证），lan-win、lan-mac、lan-ubuntu 带桌面走有头与附着测试；lan-ubuntu（Linux NUC，全运行时）、lan-linux（Linux server，备用端点）、lan-mac（macOS arm64），真实环境测试验收；验收按需向 ohmycloud 总台要端点测试支撑。本仓实操：wsl 直跑门禁（aidoc 投影在本端重生成，主机视图 `--retarget` 定标 2026-09-16；Windows 编译面是 win-gnu 非 msvc，用户裁定，CI docs 岗随迁 ubuntu）；`ssh lan-ubuntu` / `ssh lan-mac` 的 `~/browse-rs` 是 rsync 副本非 git 仓，先 rsync 源树再跑门禁。远端 origin = github.com/raystyle/browse_rs（2026-09-21 由 browse-rs 改名，旧 URL GitHub 恒 redirect），Rust 三岗 CI 已挂（linux + win-gnu 交叉 + mac，.github/workflows/ci.yml）
- 连接姿势（口径全文见 evo-adr:code-kit 的 env-platform 第十节）：WSL 到宿主恒走 127.0.0.1 回环 ssh 与 interop 直调（`cargo.exe`、`/mnt/c` 互访），不走宿主 mesh IP（mirrored 网络下自连被 RST 属结构性，非配置可修）；lan 三端（lan-ubuntu / lan-linux / lan-mac）mesh 地址互访随时可用；连接问题先查姿势再查配置
- 运维与验收脚本载体（口径见 env-platform 第十一节）：统一走 pwsh 一份（五端 pwsh 7.6.6 在位）；本仓既有跨平台脚本载体是 PEP 723 Python 经 uv（`tools/`），不强制迁移，验收与运维面新增脚本一律 pwsh
- chrome 产物验收分工（用户令 2026-09-17，clean-chrome 工位转达）：clean-chrome 编译自证止于产物在位与自检冒烟；产物验收测试（导入、驱动、真实环境面）自下编译窗起归本工位，随窗执行
