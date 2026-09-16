---
name: browse
description: "Drive clean-chrome via the browse CLI with JS-dialect snippets. Run `browse --help` for usage details."
requires_bin: browse
command: browse
---

给 agent 的 browse CLI 驾驶术：方言片段（多语句、`;` 可选）驱动 clean-chrome；变量跨调用持久；错误带可照抄的下一步。

# listPageTargets

`listPageTargets()`

列可附着 page targets（带 own 标：只有 own=true 可 closeTab）。

```js
const tabs = await listPageTargets()
```

# resolveWsUrl

`resolveWsUrl(opts?)`

把 wsUrl/port/profileDir 线索解析成 WS URL（不连接）。

## Inputs

```json
{
  "properties": {
    "opts": {
      "description": "缺省 {port:9222}",
      "type": "object"
    }
  },
  "required": [],
  "type": "object"
}
```

```js
return await resolveWsUrl({port: 9222})
```

# detectBrowsers

`detectBrowsers()`

探测本机已开的调试浏览器（profile/端口/wsUrl）。

```js
return await detectBrowsers()
```

# cdpMethods

`cdpMethods(domain?)`

652 个 CDP 命令的运行时探针（按域过滤）。

## Inputs

```json
{
  "properties": {
    "domain": {
      "description": "缺省 全部",
      "type": "string"
    }
  },
  "required": [],
  "type": "object"
}
```

```js
return await cdpMethods("Network")
```

# snapshot

`snapshot()`

AX 树快照：nodes 带 role/name/value/短 ref（e1、e2…），引用表的唯一来源。

```js
const s = await snapshot()
```

# screenshot

`screenshot(path?, full?)`

页内截图存 PNG，回 {path,bytes}。

## Inputs

```json
{
  "properties": {
    "full": {
      "description": "缺省 false",
      "type": "boolean"
    },
    "path": {
      "description": "缺省 <state>/screenshots/…png",
      "type": "string"
    }
  },
  "required": [],
  "type": "object"
}
```

```js
return await screenshot()
```

# pdf

`pdf(path?)`

当前页存 PDF（仅无头 chrome），回 {path,bytes}。

## Inputs

```json
{
  "properties": {
    "path": {
      "description": "缺省 <state>/pdfs/…pdf",
      "type": "string"
    }
  },
  "required": [],
  "type": "object"
}
```

```js
return await pdf()
```

# newTab

`newTab(url?)`

开新 tab 并设为活动路由（先 about:blank 再 goto，防竞速假完成）。

## Inputs

```json
{
  "properties": {
    "url": {
      "description": "缺省 about:blank",
      "type": "string"
    }
  },
  "required": [],
  "type": "object"
}
```

```js
await newTab("https://example.com")
```

# switchTab

`switchTab(targetId)`

切活动路由（不改 Chrome 可见前景，人机共存）。

## Inputs

```json
{
  "properties": {
    "targetId": {
      "description": "",
      "type": "string"
    }
  },
  "required": [
    "targetId"
  ],
  "type": "object"
}
```

```js
await switchTab(tabs[0].targetId)
```

# currentTab

`currentTab()`

当前活动 tab 简表（无活动返回 null）。

```js
return await currentTab()
```

# closeTab

`closeTab(targetId?)`

关 tab；守卫只放行自建 tab（chrome 初始页与用户 tab 拒绝）。

## Inputs

```json
{
  "properties": {
    "targetId": {
      "description": "缺省 当前活动 tab",
      "type": "string"
    }
  },
  "required": [],
  "type": "object"
}
```

```js
await closeTab((await currentTab()).targetId)
```

# clickAt

`clickAt(x, y)`

视口坐标 trusted 点击（点当前可见物，不做遮挡检查）。

## Inputs

```json
{
  "properties": {
    "x": {
      "description": "",
      "type": "number"
    },
    "y": {
      "description": "",
      "type": "number"
    }
  },
  "required": [
    "x",
    "y"
  ],
  "type": "object"
}
```

```js
await clickAt(120, 40)
```

# fillInput

`fillInput(selector, text)`

按 CSS 选择器填输入框（SelectAll 不发 Ctrl+A，回读严格验证；select 走 selectOption）。

## Inputs

```json
{
  "properties": {
    "selector": {
      "description": "",
      "type": "string"
    },
    "text": {
      "description": "",
      "type": "string"
    }
  },
  "required": [
    "selector",
    "text"
  ],
  "type": "object"
}
```

```js
await fillInput("#q", "hello")
```

# clickRef

`clickRef(ref)`

按 snapshot 短 ref 点击：滚动可见、量中心、遮挡命中测试（被盖即拒绝并报遮挡物）、trusted 派发。

## Inputs

```json
{
  "properties": {
    "ref": {
      "description": "",
      "type": "string"
    }
  },
  "required": [
    "ref"
  ],
  "type": "object"
}
```

```js
await clickRef("e3")
```

# fillRef

`fillRef(ref, text)`

按 ref 填输入框：objectId focus、SelectAll+insertText、同节点回读严格验证。

## Inputs

```json
{
  "properties": {
    "ref": {
      "description": "",
      "type": "string"
    },
    "text": {
      "description": "",
      "type": "string"
    }
  },
  "required": [
    "ref",
    "text"
  ],
  "type": "object"
}
```

```js
await fillRef("e2", "hello")
```

# selectOption

`selectOption(ref, value)`

下拉框选择（value 或可见 label；设值+派发 input/change；未命中报全部可选值）。

## Inputs

```json
{
  "properties": {
    "ref": {
      "description": "",
      "type": "string"
    },
    "value": {
      "description": "",
      "type": "string"
    }
  },
  "required": [
    "ref",
    "value"
  ],
  "type": "object"
}
```

```js
return await selectOption("e4", "Beta")
```

# pressKey

`pressKey(key)`

按一个键（Enter/Tab/单字符；Enter 的 text 是 \r，CDP 契约）。

## Inputs

```json
{
  "properties": {
    "key": {
      "description": "",
      "type": "string"
    }
  },
  "required": [
    "key"
  ],
  "type": "object"
}
```

```js
await pressKey("Enter")
```

# dialogStatus

`dialogStatus()`

当前 JS 对话框状态（open/type/message/defaultPrompt）。

```js
return await dialogStatus()
```

# dialogAccept

`dialogAccept(text?)`

接受当前对话框（prompt 可带应答文本）。

## Inputs

```json
{
  "properties": {
    "text": {
      "description": "",
      "type": "string"
    }
  },
  "required": [],
  "type": "object"
}
```

```js
await dialogAccept("yes")
```

# dialogDismiss

`dialogDismiss()`

拒绝当前对话框。alert/beforeunload 会被自动接受，无需手动。

```js
await dialogDismiss()
```

# routeBlock

`routeBlock(pattern)`

网络拦截：glob 命中的请求直接失败（BlockedByClient）。作用于当前活动 tab。

## Inputs

```json
{
  "properties": {
    "pattern": {
      "description": "",
      "type": "string"
    }
  },
  "required": [
    "pattern"
  ],
  "type": "object"
}
```

```js
await routeBlock("*://ads.example.com/*")
```

# routeMock

`routeMock(pattern, body, opts?)`

网络拦截：命中的请求本地应答（默认带 ACAO *）。

## Inputs

```json
{
  "properties": {
    "body": {
      "description": "",
      "type": "string"
    },
    "opts": {
      "description": "缺省 {status:200, contentType:\"text/html\"}",
      "type": "object"
    },
    "pattern": {
      "description": "",
      "type": "string"
    }
  },
  "required": [
    "pattern",
    "body"
  ],
  "type": "object"
}
```

```js
await routeMock("http://mock.test/api*", "{\"ok\":1}", {contentType: "application/json"})
```

# routeClear

`routeClear()`

清空全部拦截规则并 Fetch.disable。

```js
await routeClear()
```

# waitLoad

`waitLoad(ms?)`

等 document.readyState 到 complete（已加载立即返回）。

## Inputs

```json
{
  "properties": {
    "ms": {
      "description": "缺省 10000",
      "type": "number"
    }
  },
  "required": [],
  "type": "object"
}
```

```js
await waitLoad(8000)
```

# waitIdle

`waitIdle(ms?)`

等 network 静默（窗口语义：起点前挂着的请求不计）。

## Inputs

```json
{
  "properties": {
    "ms": {
      "description": "缺省 10000",
      "type": "number"
    }
  },
  "required": [],
  "type": "object"
}
```

```js
await waitIdle(5000)
```

# recordStart

`recordStart(opts?)`

开始录屏（Screencast 帧流落盘；everyNthFrame 源端抽帧、maxWidth/maxHeight 限宽高）。

## Inputs

```json
{
  "properties": {
    "opts": {
      "description": "缺省 {everyNthFrame:1}",
      "type": "object"
    }
  },
  "required": [],
  "type": "object"
}
```

```js
return await recordStart({everyNthFrame: 2})
```

# recordStop

`recordStop()`

停止录制，回 {frames,bytes,dir}（PNG 已在盘上）。

```js
return await recordStop()
```

# hostFunctions

`hostFunctions()`

本目录的运行时探针（agent 自描述，schema/清单同源）。

```js
return await hostFunctions()
```

# print

`print(x)`

把值打到 daemon stderr（调试用）。

## Inputs

```json
{
  "properties": {
    "x": {
      "description": "",
      "type": "any"
    }
  },
  "required": [
    "x"
  ],
  "type": "object"
}
```

```js
await print(tabs)
```

完整面（含 session 方法与 CLI 形态）见 llms-full.txt 与 browse.schema.json。
