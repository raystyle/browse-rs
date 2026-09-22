# /eval HTTP API 公开契约（#45）

> browse 的 daemon 本就是一个 HTTP 面（ADR-0001）。本档是它的公开契约：方言做线协议，门外语言不限（curl、PowerShell、Bash、Python、Node、agent 本体都可以直接驱动），零 SDK 依赖；各语言薄封装只是可选便利层，不是官方路线。

## 端点

| 方法 | 路径 | 用途 |
|---|---|---|
| POST | `/eval` | 求值一段方言片段（或 `js: true` 时全量 JS） |
| GET | `/health` | 探活与状态（就绪即 200，响应形见下） |
| POST | `/engine/up` | 显式起引擎 |
| POST | `/quit` | 退 daemon（只杀自起引擎） |

## GET /health 响应形

```json
{ "ok": true, "name": "default", "uptime": 1656, "connected": true,
  "activeTargetId": "46BC…", "activeSessionId": "FF6D…",
  "daemon": { "os": "linux", "hostname": "AI-LAB", "pid": 2085366,
      "name": "default", "startedAt": 1790097761849 },
  "engine": { "Spawned": { "pid": 267718, "ws_url": "ws://127.0.0.1:43877/…",
      "profile_dir": "…", "chrome": "…", "headless": false, "channel": "port",
      "spawned_at": 1790097761849 } },
  "engineProvenance": { "origin": "managed-spawn", "hostContext": "linux/AI-LAB",
      "spawnedBy": "daemon", "spawnedAt": 1790097761849 } }
```

`engine` 三态（单键包裹的枚举形）：`{"Spawned":{pid,ws_url,profile_dir,chrome,headless,channel,spawned_at}}`（自起）、`{"Attached":{ws_url}}`（附着）、`{"NotConnected":null}`。`daemon` 是 daemon 自描述（os/hostname/pid/name/startedAt）；`engineProvenance` 是来源语义面（origin 枚举 attached/managed-spawn/isolated-spawn、hostContext、spawnedBy、spawnedAt；attached 态 loopback 宿主显式降级标注）。门外 agent 用它判「引擎在不在、活动 tab 是哪个、引擎在谁的宿主上」。

## POST /engine/up 请求体

字段全可省（走发现序）：`headless`（bool）、`chrome`（路径）、`ws`（钉死 WS URL）、`port`、`pipe`（bool）、`profile`、`proxy`、`proxy_bypass`、`isolated`。失败同 `/eval` 的 500 加错误形。

## POST /eval

请求体（JSON）：

```json
{ "code": "return await currentTab()", "new_tab": false, "js": false }
```

- `code`：方言片段（缺省）或 JS 源码（`js: true`，等价 CLI 的 `--js` 面）
- `new_tab`：先开 about:blank 新 tab 再求值（等价 `--new-tab`）
- `js`：走全量 JS 受限旁路（#22 口径）

响应（成功，HTTP 200）：

```json
{ "ok": true, "value": { "targetId": "A1B2…", "title": "Example", "url": "https://example.com/" } }
```

响应（失败，HTTP 500 带错误形；判 `ok` 字段仍是推荐姿势，但客户端要允许从非 2xx 响应读出 body，别用 curl -f 一类「非 2xx 即抛」姿势把 CTA 吞掉）：

```json
{ "ok": false, "error": "方言不支持该语法（if/for/while…）；下一步：…" }
```

错误形带下一步 CTA（browse 全家错误面的统一口径），驱动方可以直接把它转给人或 agent。

## 语义要点

- **单飞槽**：daemon 同一时刻只跑一段求值（eval_lock），并发 POST 按到达序排队执行、不拒。`BROWSE_EVAL_TIMEOUT` 只管自己的求值段（拿到槽之后才起算），排队等待另计：第 N 个并发请求的总时延约等于前 N-1 个耗时之和加自己的求值，排队期间不会超时。客户端要按最坏 N 乘 timeout 设自己的 HTTP 超时，别把排队误判成 daemon 挂了。
- **超时**：单次求值上限 `BROWSE_EVAL_TIMEOUT`（秒，缺省 300），超时回错误形（`eval 超时（N 秒；BROWSE_EVAL_TIMEOUT 可调）`）。
- **懒引擎**：引擎未连且片段不含 connect 时自动拉起（懒拉起），冷启动首求值包含 spawn 时延。
- **状态跨请求保留**：`const` 变量与引用表在 daemon 生命期跨片段持久（多段工作流直接分段 POST）。
- **daemon 发现**：端口 `BROWSE_PORT`（缺省 9880）；命名实例 `BROWSE_NAME` 派生端口 9900-9999。客户端直接 POST 即可：没有 daemon 时 CLI 会自动拉起，但纯 HTTP 客户端需先 `browse up`（或自起 `browse --serve`）。

## 兼容承诺

契约即 HTTP + JSON：只加字段不减字段；错误响应当前恒为 HTTP 500，未来若细化状态码（如 400 参数错、503 引擎不可用），body 形仍恒为 `{ok:false, error}`，客户端只需认 body 不受状态码变化打穿。任意脚本语言可驱动。

## curl 三例

```bash
# 端口缺省 9880（BROWSE_PORT）；命名实例（BROWSE_NAME）派生 9900-9999，实际端口见 browse status

# 1. 探活
curl -s http://127.0.0.1:9880/health

# 2. 求值：navigate + 取 title
curl -s http://127.0.0.1:9880/eval \
  -H 'Content-Type: application/json' \
  -d '{"code": "await goto(\"https://example.com\"); return (await session.Runtime.evaluate({expression:\"document.title\", returnByValue:true})).result.value"}'

# 3. 取快照（限深省 token）
curl -s http://127.0.0.1:9880/eval \
  -H 'Content-Type: application/json' \
  -d '{"code": "const s = await snapshot({depth: 2}); return {url: s.url, nodes: s.nodes.length}"}'
```

## 与片段库的组合（#44）

外置驱动器读片段文件 POST `/eval`，即成「缺 helper 自己存」的闭环：方言做线协议，门外语言不限。

## 测试用例（验收面）

- curl 驱动全链路：`browse up` 后探活、navigate、snapshot 取回（三例各一）
- 错误形：POST 一段语法错误片段，回 HTTP 500 且 body `ok: false`、`error` 带 CTA
- 超时语义：`BROWSE_EVAL_TIMEOUT=1` 下 POST 一段 5 秒等待，1 秒级回错误形
