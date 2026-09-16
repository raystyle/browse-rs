# Changelog

版本级里程碑；逐批过程见 docs/diary。semver 判据在册 docs/requirements/REQ-004。

## 0.3.0 - 2026-09-17

- 自定义引擎 profile：`browse up --profile <dir>` 与 `BROWSE_PROFILE`（spawn 引擎的 user-data-dir；显式旗标顶掉环境缺省）。默认仍是固定 `<state>/engine-profile`，站点状态与会话跨跑持久（down 不删）
- EngineSpec::Auto 增 profile 字段（HTTP `/engine/up` 面同构透传）；WS 与管道两通道 spawn 同支持

## 0.2.1 - 2026-09-17

- 修 attach 断线不愈：Session 存活旗原先只由读循环写进局部对象，连接断开后 `is_connected` 永远谎报活着，引擎懒 ensure 短路不重连，一切求值 `CDP socket closed` 直到重启 daemon；改共享旗后断线即翻 false，下一次求值自动重连或重起引擎（WS 与管道两通道同修，附回归测试与杀引擎自愈实证）
- 观察入账：懒 ensure 重起引擎用缺省形态（headless 随缺省），显式 headless 需再 `browse up --headless`

## 0.2.0 - 2026-09-17

- Chromium 版本管理器 R2 下载腿：`browse chrome install <版本>`（部署目录缺省）与 `chromeInstall({version})` 从 chrome.ohmygh.com 版本段下载，`.sha256` 边车锚校验后 zip 解包原子落位，manifest 登记自动 pin；`BROWSE_CHROME_MIRROR` / `BROWSE_CHROME_ASSET` 覆写（资产名暂定约定，候首版资产定标）
- `browse --version`（资产解包冒烟用；原计划 0.1.1 patch 并入本批）

## 0.1.0 - 2026-09-16

- 首版封口：三层 crate（cdp 协议层、browse-core 引擎与方言宿主、browse-cli）加常驻 daemon（HTTP 默认 9880 与命名实例派生端口）
- 方言片段三形态求值（参数、stdin、TTY REPL），附着优先引擎策略（探测 9222 缺则 spawn clean-chrome 隔离实例，down 只杀自起）
- 命令面单一真相源目录与三投影（schema、llms、llms-full），agent 发现通道 `browse --llms` 三形态直出（--full/--json）
- 内嵌 Chromium 版本管理器本地导入面（chrome install/use/list/doctor，ADR-0007；R2 下载腿候 omc 端点热验）
- 域策略防线（BROWSE_DENY_DOMAINS/ALLOW_DOMAINS）、CDP 管道通道（--pipe 零 TCP 面）、652 方法清单派生
- CI 三岗（linux、win-gnu 交叉、mac）加文档岗；Windows 编译面定标 win-gnu（非 msvc）
