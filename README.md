# browse

[![CI](https://github.com/raystyle/browse-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/raystyle/browse-rs/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/raystyle/browse-rs)](https://github.com/raystyle/browse-rs/releases)
[![License](https://img.shields.io/badge/license-MIT_OR_Apache--2.0-blue)](LICENSE-MIT)

## 项目介绍

browse 是给 agent（也给人）的浏览器驾驶 CLI：一条 JS 方言片段驱动
clean-chrome（自编 Chromium），常驻 daemon 让会话、变量、活动 tab 与元素
引用跨命令存活。错误自带下一步（可照抄 CTA），绝不关用户自己的浏览器。

- 一条 JS 方言片段直调 CDP 全量 652 方法（`session.<Domain>.<method>`）
- 常驻 daemon：会话、变量、活动 tab 与元素引用跨命令存活
- 页面级原语：snapshot/screenshot/pdf、clickRef/fillRef、路由拦截与假应答
- 附着优先引擎策略：探测本机浏览器，缺则 spawn 隔离实例；绝不关用户浏览器
- 内嵌 Chromium 版本管理器：install/use/list/doctor，镜像锚校验原子落位
- agent 面：`--llms` 手册直出、错误带可照抄「下一步」、裸调用不弹交互

与家族分工：omc 管资源分发与总台协调；ark 管舰队执行与安装管理；browse
自管浏览器引擎与会话（含 Chromium 版本管理器，ADR-0007）。

边界（用户裁定，勿再提议）：不做 MCP；控制流与函数不进方言（ADR-0002）；
不做 Cloud browser。

## 部署

三条通道：

1. ark install（舰队安装管理，单通道契约：无自升级子命令，升级走管理方或重下；
   catalog 入册随总台 catalog pin 滚动，未入册前走下两通道）
2. 镜像直下：`https://browse.ohmygh.com/browse/<版本>/<资产>`，三平台包
   加同名 `.sha256` 边车（sha256sum 原生格式，下载后 `sha256sum -c` 核验）：

   | 平台 | 资产 |
   | --- | --- |
   | Linux x86_64 | `browse-v<版本>-x86_64-unknown-linux-gnu.tar.gz` |
   | Windows x86_64 | `browse-v<版本>-x86_64-pc-windows-gnu.zip` |
   | macOS arm64 | `browse-v<版本>-aarch64-apple-darwin.tar.gz` |

3. 源码：`cargo install --path crates/browse-cli --force`

五端注意：Windows（win-gnu 交叉构建，CRT 静态零 DLL 依赖）；Linux 最小
系统补 `libnss3 libasound2t64` 族；macOS arm64。装好后首条命令自动拉起
daemon。

前置引擎：`browse chrome install <版本>` 从镜像装 clean-chrome（sha256
边车锚校验后原子落位），或本地导入部署目录。

## 配置

环境变量：

| 变量 | 作用 |
| --- | --- |
| `BROWSE_PORT` | daemon 端口（默认 9880） |
| `BROWSE_NAME` | 命名实例：状态目录加派生端口 9900-9999（ADR-0006） |
| `BROWSE_CHROME` | chrome 路径（缺省走发现序：显式 -> 托管 pin -> 祖先部署 -> 常规路径） |
| `BROWSE_PROFILE` | spawn 引擎自定义 user-data-dir（默认固定 engine-profile 持久会话） |
| `BROWSE_CDP_WS` | 钉死连接的 WS URL |
| `BROWSE_NO_ATTACH=1` | 跳过附着探测，强制 spawn 隔离实例 |
| `BROWSE_NO_AUTO_DIALOG=1` | 关掉 alert 自动接受 |
| `BROWSE_EVAL_TIMEOUT` | 单次求值超时秒数（默认 300） |
| `BROWSE_DENY_DOMAINS` / `BROWSE_ALLOW_DOMAINS` | 域策略（后缀匹配，deny 优先） |
| `BROWSE_CHROME_MIRROR` / `BROWSE_CHROME_ASSET` | 版本管理器镜像与资产名覆写 |
| `BROWSE_ISSUES_API` | issue 通道基址覆写（测与灰度） |

状态目录 `~/.browse-rs/`（Windows `%USERPROFILE%\.browse-rs`，命名实例在
`<name>/` 子目录）：`daemon.log`、`engine.log`、`engine-profile`（down
不删，复用登录态）、`chromium/`（托管引擎版本段加 manifest.json）、
`screenshots/`、`pdfs/`、`record-*/`、`drops/`（超 32KB 大结果自动落盘）。

密钥纪律：browse 本体无内置密钥；镜像推流 token 只在 CI Secrets；issue
通道匿名提交（每 IP 每时 10 条）。

## 使用方法

```bash
browse up --headless                                  # 起无头引擎（附着优先）
browse 'await newTab("https://example.com")'          # 开页面
browse 'return await snapshot()'                      # AX 树快照（节点带短引用）
browse 'await fillRef("e2", "hello")'                 # 按引用填输入
browse 'await clickRef("e3")'                         # 按引用点击（遮挡守卫）
browse 'return await screenshot()'                    # 截图；pdf()/recordStart() 同族
browse 'await routeBlock("*://ads.example.com/*")'    # 拦网；routeMock 本地假应答
browse 'return await detectBrowsers()'                # 探测可附着浏览器
browse chrome install 152.0.7977.84                   # 镜像装引擎（或本地导入）
browse issue new "标题" --body "复现步骤"              # 缺陷一键反馈（自动署名）
browse down                                           # 幂等退场（附着来源不动）
```

agent 手册一行：`browse --llms`（markdown 手册；`--full` 完整目录、
`--json` 机器形 Schema）。缺陷反馈一行：`browse issue new`。

方言边界：无运算符与控制流（页面逻辑放 `Runtime.evaluate` 的
expression，页内是真 V8）；CDP 全量 652 方法 `session.<Domain>.<method>`
直调。错误三段式自带下一步；退出码 0/1/2（成功/失败/用法错）。

开发面：`cargo test --workspace`；`BROWSE_E2E=1 BROWSE_NO_ATTACH=1 cargo
test -p browse-core --test e2e`。深入读 AGENTS.md（工程契约）、
docs/architecture.md、docs/adr/、docs/surface/（命令面派生物）。
