# browse

给 agent（也给人）用的浏览器驾驶 CLI：一条 JS 方言片段驱动你的
[clean-chrome](https://github.com/raystyle/clean-chrome)（自编 Chromium），
常驻 daemon 让会话跨命令存活。

```bash
browse 'await newTab("https://example.com")'   # 打开页面
browse 'return await snapshot()'               # 拿页面结构（节点带 e1/e2 短引用）
browse 'await clickRef("e3")'                  # 按引用点击
```

为什么顺手：

- **会话常驻**：变量、活动 tab、元素引用表跨命令存活，一条命令做一步
- **错误自带下一步**：每条报错都附可照抄的 CTA（下一步做什么），不让你猜
- **安全守卫**：绝不关你自己的浏览器，只关自己开的 tab；点击前查遮挡、引用过期即拦
- **双通道**：WebSocket 或 clean-chrome 管道模式（`--pipe`，零 TCP 面）
- **三平台真机验证**：Windows / macOS / Linux 双通道端到端全绿

## 安装

前置：本机有 clean-chrome 部署（本仓库根的 `chromium-*/chrome.exe`，
或设 `BROWSE_CHROME` 环境变量指向任意 chrome.exe）。

```bash
git clone <本仓库> && cd browse-rs
cargo install --path crates/browse-cli --force
browse --help
```

装好后第一条任意命令会自动拉起常驻 daemon（日志在
`~/.browse-rs/daemon.log`）；`browse down` 幂等退出。

## 五分钟上手

### 1. 打开页面，读点东西

```bash
browse up --headless                 # 起一个无头引擎（附着优先，缺则自起）
browse 'await newTab("https://example.com")'
browse 'return await waitLoad()'
browse 'return (await session.Runtime.evaluate({expression:"document.title", returnByValue:true})).result.value'
# example.com
```

不想无头？`browse up` 直接开有头窗口，人机共存：自动化不抢你的前台。

### 2. 快照、点击、填表（元素引用流）

```bash
browse 'return await snapshot()'                # AX 树：每节点带短引用 ref
browse 'await fillRef("e2", "hello rust")'      # 按引用填输入框（回读验证）
browse 'await clickRef("e3")'                   # 按引用点击（遮挡守卫）
browse 'return await selectOption("e4", "Beta")'  # 下拉框（value 或可见 label）
```

引用比 CSS 选择器稳：页面重构不漂移，导航后过期会被拦下并提示重新
snapshot，绝不静默点错位置。

### 3. 留档：截图 / PDF / 录屏

```bash
browse 'return await screenshot()'              # PNG，回 {path,bytes}
browse 'return await pdf()'                     # PDF（仅无头）
browse 'return await recordStart()'             # 开始录屏（帧流落盘）
browse '...操作页面...'
browse 'return await recordStop()'              # 回 {frames,bytes,dir}
```

### 4. 等待与对话框

```bash
browse 'await waitLoad(8000)'                          # 等 readyState
browse 'await waitIdle(5000)'                          # 等 network 静默
browse 'await session.waitJs("window.done", 5000)'     # 等页内条件（真 V8）
browse 'return await dialogStatus()'                   # 有 confirm/prompt？
browse 'await dialogAccept()'                          # 显式应答（alert 会自动接受）
```

### 5. 拦网（测试与反打扰）

```bash
browse 'await routeBlock("*://ads.example.com/*")'                      # 拦死
browse 'await routeMock("http://mock.test/api*", "{\"ok\":1}")'         # 本地假应答
browse 'await routeClear()'
```

### 6. 附着你自己的浏览器 / 多实例

```bash
browse 'return await listPageTargets()'     # 你开着 clean-chrome（9222）时直接附着
BROWSE_NAME=work browse up --headless       # 命名实例：独立端口与状态目录，并行互不干扰
browse down
```

## 命令速查

| 形态 | 说明 |
| --- | --- |
| `browse '<片段>'` | 求值（多语句、`;` 可选，`return` 出值，变量跨调用持久） |
| `browse -e '<片段>'` / stdin / TTY REPL | 其余两传输形态（括号配平批处理 / 交互） |
| `browse up [--headless] [--pipe]` | 显式起引擎 |
| `browse down` / `browse status [--json]` | 退出 / 看状态 |

完整命令面（含全部全局函数与 session 方法的参数、示例）：
[`docs/surface/llms-full.txt`](docs/surface/llms-full.txt)；机器可读契约
`docs/surface/browse.schema.json`；方言内运行时探针 `hostFunctions()`。

方言边界：没有运算符 / if / for / 函数（页面逻辑放进
`Runtime.evaluate` 的 `expression` 字符串，页内是真 V8）；CDP 全量
652 个方法走 `session.<Domain>.<method>(params)` 直调，拼错自动给相近建议。

## 文件落在哪

都在 `~/.browse-rs/`（命名实例在其 `<name>/` 子目录）：
`daemon.log`（daemon 日志）、`engine.log`（引擎 chrome 诊断）、
`engine-profile`（引擎 profile，down 不删、复用登录态）、`screenshots/`、
`pdfs/`、`record-*/`（录屏帧）、`drops/`（超 32KB 的大结果自动落盘，
stdout 只回路径与预览）。

## 出错怎么办

错误三段式：`诊断（行L:列C）；下一步：<可照抄的命令>`。退出码
`0` 成功 / `1` 执行失败 / `2` 用法错。常见问题：

- **daemon 没起来**：看 `~/.browse-rs/daemon.log`；`browse down` 后重试
- **引擎起不来**：`BROWSE_CHROME` 指到 chrome.exe；看 `engine.log`
- **点了没反应**：多半被遮挡或引用过期，报错里直接给下一步
- **管道挂着不返回**：`browse status` 看引擎；`browse down` 清场重来

## 环境变量

| 变量 | 作用 |
| --- | --- |
| `BROWSE_PORT` | daemon 端口（默认 9880） |
| `BROWSE_NAME` | 命名实例：状态目录 + 派生端口 9900-9999（ADR-0006） |
| `BROWSE_CHROME` | chrome.exe 路径（缺省走发现序） |
| `BROWSE_CDP_WS` | 钉死连接的 WS URL |
| `BROWSE_NO_ATTACH=1` | 跳过附着探测，强制 spawn 隔离实例 |
| `BROWSE_NO_AUTO_DIALOG=1` | 关掉 alert 自动接受 |
| `BROWSE_EVAL_TIMEOUT` | 单次求值超时秒数（默认 300） |
| `BROWSE_DENY_DOMAINS` / `BROWSE_ALLOW_DOMAINS` | 域策略（后缀匹配，deny 优先） |

## 开发

```bash
cargo test --workspace                                            # 单元 + 契约
BROWSE_E2E=1 BROWSE_NO_ATTACH=1 cargo test -p browse-core --test e2e   # 真 chrome 端到端
cargo clippy --workspace --all-targets -- -D warnings
```

深入读：`AGENTS.md`（工程契约）、`docs/architecture.md`（怎么拼）、
`docs/guides/getting-started.md`（上手细节）、`docs/adr/`（为什么）、
`docs/aidoc/llms.txt`（Rust API 索引）、`docs/surface/`（命令面派生物）。

Roadmap 五项（元素引用、录制、代际失效、多实例、POSIX 管道）已全部
落地；后续吸收面以 `docs/adr/` 与提交记录为准。

## 明确不做（用户裁定，勿再提议）

- MCP server（接口面就是 CLI + 方言）
- 秘密脱敏（BROWSE_REDACT 输出层掩码）
- 模型循环/观察循环（消费者是编码 agent，它自带循环与视觉）
- 控制流/函数进方言（ADR-0002）
- Cloud browser（与 clean-chrome 本地优先哲学相反）
