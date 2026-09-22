# getting-started：五分钟上手

前置：本机有 clean-chrome 部署（仓库根 `chromium-*/chrome.exe`，或设 `BROWSE_CHROME`）。

## 1. 装

```bash
cargo install --path crates/browse-cli --force
browse --help
```

## 2. 一条命令开跑（自动拉 daemon + 引擎）

```bash
browse --headless 'await session.Page.navigate({url:"data:text/html,<title>hi</title>"} )'
browse 'return (await session.Runtime.evaluate({expression:"document.title", returnByValue:true})).result.value'
# "hi"（字符串带引号，与对象输出可区分，#21）
```

- `--headless` 只影响**首次** spawn 的引擎形态；daemon 常驻，后续调用免拉起。
- 无显示会话（无 `DISPLAY`/`WAYLAND_DISPLAY`，典型 ssh）下 spawn 有头意图自动回退补 `--headless`（#57；`BROWSE_ENGINE_ARGS` 里的显式 headless 旗标优先不叠补）；daemon 常驻改环境需重启（在册口径）。
- 变量跨调用持久：`browse 'const tabs = await listPageTargets()'` 之后另起进程 `browse 'return tabs[0].url'` 仍可用。

## 3. 真网页

```bash
browse up --headless            # 或 browse up（有头窗口）
browse 'return await goto("https://example.com")'   # navigate+等加载一体，回 {url,title,elapsedMs}
browse 'return (await session.Runtime.evaluate({expression:"document.querySelector(\"h1\").textContent", returnByValue:true})).result.value'
```

> 等加载优先 goto() 或 waitLoad(s)；`waitFor("Page.loadEventFired")` 有竞速窗（事件在注册前已发则假超时）。wait 类 timeout 一律秒口径（#51）。

## 4. 管道通道（零 TCP 面）

```bash
browse up --headless --pipe
browse status     # engine 行 channel=pipe；此时 9222 无监听
browse down
```

## 5. 附着已在跑的浏览器

两个实体的分界：**用户 chrome** 是你自己开的浏览器（本生态常见形是双击 clean-chrome，无参数即开 9222，免确认对话框），**集成 chrome** 是 browse spawn 管理的引擎实例（版本部署归 `browse chrome` 命令族，固定 engine-profile 持久登录态）。引擎策略附着优先：探到用户 chrome 的调试口就在它里面干活（当前引擎 = 它），探不到才 spawn 集成 chrome；`BROWSE_NO_ATTACH=1` 钉死 spawn 态。方言函数不区分两者，都落在「当前引擎」的活动 tab。

用户 chrome 开着调试口后：

```bash
browse 'await listPageTargets()'          # 探测附着，不新开浏览器
browse 'await session.use((await listPageTargets())[0].targetId)'
```

跨体借登录态：`browse 'cloneCookies(["github.com"])'` 从用户 chrome 只读热迁到集成引擎（源零写回；探测面只认 9222 与默认 profile 调试口，其他端口的源走 exportStorageState/importStorageState 往返）。

铁律：附着来源绝不关浏览器、绝不关非自建 tab（守卫在 `Session::call` 层强制）。

## 6. 元素引用与录制（不写选择器）

```bash
browse 'const s = await snapshot()'          # nodes[].ref 是短引用（e1、e2…）
browse 'await clickRef("e3")'                # 滚动可见->trusted 点击
browse 'await fillRef("e2", "hello rust")'   # 填输入框并回读验证
browse 'return await recordStart({everyNthFrame:2})'   # 录帧开始（可选抽帧/限宽）
browse 'await recordStop()'                  # -> {frames,bytes,dir}（PNG 已落盘）
```

导航后旧 ref 失效（代标记主动拦 + resolveNode 被动兜底），错误自带
「重新 snapshot」CTA：重新 `snapshot()` 拿新 ref 即可。

## 7. 多实例（BROWSE_NAME）

```bash
BROWSE_NAME=work browse up --headless        # 独立端口(9900-9999)与状态目录
BROWSE_NAME=work browse 'return 1+1'         # 与默认实例互不可见
browse status                                # 默认实例不受影响
BROWSE_NAME=work browse down
```

## 8. 批处理（stdin）

```bash
printf '%s\n' \
  'const t = await listPageTargets()' \
  'return t[0].type' \
| browse
```

## 9. 版本管理（自更新与引擎）

```bash
browse update                        # browse 自更新（stable 段双通道加锚校验；0.6.1 前无此命令）
browse chrome update                 # 引擎升最新并默认切用（发现源 latest；BROWSE_CHROME_LATEST 可钉）
browse chrome list                   # 已装版本与当前 pin
browse chrome use 152.0.7977.84      # pin 切换（旧版保留可回退）
browse chrome remove 152.0.7977.84   # 删旧版（pin 指向的拒删，先 use 切走）
browse chrome doctor                 # 部署体检（在位/文件基线/pin）
```

## 10. 站点与机制知识（workspace 技能仓）

```bash
browse workspace install             # git clone 知识仓到 ~/.browse-rs/workspace（BROWSE_WORKSPACE 可改路径）
browse workspace update              # git pull --ff-only（本地有未提交修改会拒绝）
browse workspace list                # 列全部站点段与页面特征 slug
browse workspace site github         # 读单站知识清单；site github/scraping.md 读全文
browse workspace page captcha        # 读机制配方全文
```

goto 会自动点名：命中站点知识或页面特征（iframe、验证码、水合等）时回执附
`domain_skills`/`page_skills` 清单与 hint 字段，照 hint 拉全文即可；fetch 回执
按 URL 同口径点名站点知识（两腿都生效，不依赖引擎），detect() 的
challenged/login-wall 判读点名对应机制配方；两层可用
`BROWSE_DOMAIN_SKILLS=0` / `BROWSE_PAGE_SKILLS=0` 独立关闭。

## 常见坑

- 方言没有 `if/for/函数`：页面逻辑写进 `Runtime.evaluate` 的 `expression` 字符串（页内是真 V8）。模板字符串有：反引号 raw 语义，`\n`、`\d` 等逐字保留、可多行，页面代码直接内嵌。
- 多语句片段要 `return` 才有输出（与样例一致）。
- 大结果（>32KB）自动落盘：stdout 回 `{"__dropped":true,"path":…,"preview":…}`，按 path 取全量。
- E2E 测试共享 profile 会撞 chrome 单实例锁；测试已内置串行（跑法带 `BROWSE_NO_ATTACH=1`，防误附着你的浏览器）。
- daemon 日志在 `<state>/daemon.log`（命名实例在 `~/.browse-rs/<name>/`）；`browse down` 幂等。
