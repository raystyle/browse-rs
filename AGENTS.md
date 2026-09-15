# browse-rs

Rust workspace（crates/cdp、browse-core、browse-cli）。公开契约以 `///` 与类型签名为准；toolchain 见 rust-toolchain.toml（1.98.0）。仓库根的 `chromium-*/` 是 clean-chrome 部署产物，已 .gitignore，勿动。

## Commands

- cargo fmt --all
- cargo clippy --workspace --all-targets -- -D warnings
- cargo test --workspace
- cargo test --doc --workspace
- cargo doc --no-deps --workspace
- cargo aidoc --check --strict   # aidoc 投影漂移门禁（改动 pub/文档后先 cargo aidoc 再提交 docs/aidoc）
- cargo install --path crates/browse-cli --force    # 本机装 browse 进 PATH
- BROWSE_E2E=1 cargo test -p browse-core --test e2e # 真 chrome 端到端（本机才有 clean-chrome；CI 跳过）

## Must

- 改 pub 项：同步 `///` 与 doctest（missing_docs 是 deny，CI 必红），并 `cargo aidoc` 后提交 `docs/aidoc/`
- 不可逆技术选择：先写 docs/adr/ 或改旧 ADR 的 Status
- 文档链接只用 intra-doc（`` [`Session::call`] ``）
- I/O 类示例标 no_run，不标 ignore
- spawn 引擎保持 `--no-sandbox`（SxS 部署沙箱打不开自身 exe，见 ADR-0003）

## Must not

- 手改 docs/aidoc/（生成物，下次 cargo aidoc 会覆盖）或另写 API.md 当第二真相
- 把 ADR 正文贴进函数文档；不整份读 llms-full.txt
- 关用户浏览器：守卫在 cdp `Session::call` 层，绕不过是特性不是 bug
- E2E 测试共享 engine-profile（chrome 单实例锁；tests 里用 SEQ 互斥串行）

## Read first

- docs/aidoc/llms.txt（Agent 入口索引）-> 相关 docs/aidoc/<crate>/<module>.md
- docs/architecture.md（现在怎么拼：三层 crate + daemon + 双通道）
- 仍不确定再打开源码 `///`
- docs/adr/ 仅在改对应决策时（0001 daemon / 0002 方言 / 0003 引擎策略 / 0004 自动附着 / 0005 管道）
- docs/guides/getting-started.md（CLI 使用面）

## 环境

- daemon 端口默认 9880（BROWSE_PORT）；chrome 发现序：BROWSE_CHROME -> cwd/exe 祖先的 `chromium-*/chrome.exe` -> 常规路径
- 域策略：BROWSE_DENY_DOMAINS / BROWSE_ALLOW_DOMAINS（后缀匹配，deny 优先），拦 Page.navigate 与 Target.createTarget
- 652 命令清单 crates/cdp/src/methods.txt 是生成物（tools/gen-cdp-methods.py，源头 refs/ 不入库）；改协议版本重跑生成再提交
- daemon 日志：`%USERPROFILE%\.browse-rs\daemon.log`
- 本机引擎 profile：`%USERPROFILE%\.browse-rs\engine-profile`（down 不删，复用登录态）
