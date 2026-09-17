# Changelog

版本级里程碑；逐批过程见 docs/diary。semver 判据在册 docs/requirements/REQ-004。

## 0.5.0 - 2026-09-18

- issue 命令集成（REQ-057 对齐单）：`browse issue new/list/show` 一键缺陷反馈入统一入口 issues.ohmygh.com，自动署名 tool/version/platform/host，`BROWSE_ISSUES_API` 覆写；agent 一键反馈纪律入 AGENTS 合同
- `--llms` 三面统一升册（REQ-060）：裸形升 markdown 紧凑 agent 手册（活树派生，行数帽 120 契约锁）、`--json` 机器形 Schema、`--full` 完整目录；README 四节重排（166 压 89 行）加镜像直下 URL 入册
- 裸调用面对齐：裸跑（TTY 与空管道）出本仓帮助体 exit 0，REPL 收显式 `--repl`，stdin 管道批处理不回归
- 发布流水自播上线（自本版起）：CI 退编译改 seed-only（release published 触发双段播种加零上传红灯），本地发布面 `tools/release.ps1`（预检五件加测试闸加三目标打包边车加三端解包冒烟加 gh 直发）为唯一正式发布口；评审回执终审修（三目标显式内层名、锚链预检五件、prerelease 过滤）随卷
- 帮助面清理：删外部冗余注记（browser-harness-js 对齐、clean-chrome 专属），节标题独立一行、正文统一两格缩进

## 0.4.1 - 2026-09-17

- R2 先验窗三端 happy-path 实证：wsl（linux-gnu 包 567 文件）、lan-win（msvc 包 499 文件）、lan-mac（arm64 包 331 文件束形）镜像装通即 spawn 驱动，doctor 健康，干净退场；REQ-003 trace 回填
- 边车解析对齐 `sha256sum -c` 兼容格式（hex 双空格文件名，取首 token 为锚；mock 测试同步锁真格式）
- 资产名升定标三元组形 `chromium-<版本>-<三元组>.zip`（总台裁二；win 对 msvc 资产名实相符）

## 0.4.0 - 2026-09-17

- macOS 平台适配：`.app` 束形导入（直指 `Chromium.app` 导入保留束形落版本目录）与布局感知二进制解析（`chrome_binary_in_dir`：认束内 `Contents/MacOS/Chromium`、版本目录包装形与裸 unix 形）；托管 pin 解析与祖先发现共用同口径
- mac 真机全绿：导入 331 文件/711MB、无头 spawn 加 evaluate、有头 spawn（ssh 直启 GUI 会话）；五端测试矩阵全活（wsl/lan-linux 无头，lan-ubuntu/lan-win/lan-mac 有头加附着）

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
