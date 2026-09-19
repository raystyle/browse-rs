# browse-core::semantic

语义层近期面：tab 族、交互三件、等待判官。

对齐 harness(py) helpers 的高频操作面，坑表教训直接落地：
- `new_tab` 先 about:blank 再 goto（带 url 与 attach 竞速 -> readyState 假完成）
- `fill_input` 清空不发 Ctrl+A（char 事件会输入字面 a），用
  `commands:['SelectAll']`；填完回读验证，失败如实报
- 不自动 `Target.activateTarget`（人机共存：不抢用户前台）；从未激活的
  后台 tab 收 Input 若挂起，错误 CTA 提示手动激活

全部函数作用于当前活动 tab（`session.use` 路由）。

## Functions

- `click_at` — 用 `Input.dispatchMouseEvent` pressed+released 在视口坐标 (x,y) 派发
- `click_ref` — 按短 ref 点击：滚动可见 -> 量视口中心 -> **遮挡命中测试** -> 复用
- `close_tab` — 关 tab（缺省关当前活动 tab）；守卫层只放行本会话自建 tab，用户 tab 一律拒绝。
- `current_tab` — 当前活动 tab 简表 `{targetId,title,url}`；无活动 tab 返回 `null`。
- `dblclick_ref` — 双击短 ref 元素（#23）：press/release 两轮，clickCount 递增成双击。
- `drag_ref` — 拖拽：源 ref 中心按下，分步移到目标 ref 中心松开（#23）。
- `emulate` — 视口与 UA 仿真档位（#24）：`{viewport:{width,height}, mobile, userAgent,
- `export_storage_state` — 导出会话态（#25.3）：cookies 全量加当前页 origin 的 localStorage。
- `fill_input` — 按 CSS 选择器填输入框：focus -> 全选（commands，不发 Ctrl+A）-> 可选
- `fill_ref` — 按短 ref 填输入框：objectId 上 focus -> 探测控件（SELECT/readOnly 拒收
- `hover_at` — 移动鼠标到视口坐标（#23）：触发 `:hover` 与悬停菜单的 mouseMoved。
- `hover_ref` — 悬停到短 ref 元素中心（#23）：触发 CSS `:hover` 与悬停菜单。
- `import_storage_state` — 导入会话态（#25.3）：吃 [`export_storage_state`] 的返回值或其落盘
- `key_raw` — 裸按键事件（#23）：keydown / keyup 按住语义（无 text，不发组合成键）。
- `new_tab` — 新开 tab 并设为活动路由。给了 `url` 则先建 about:blank 附着后再导航
- `pdf` — 当前页存 PDF（`Page.printToPDF`，`printBackground`+`preferCSSPageSize`），
- `press_key` — 用 `Input.dispatchKeyEvent` keyDown(+text)+keyUp 按一个键；Enter 的
- `select_option` — 按短 ref 选下拉框选项：value 或可见 label 匹配，设值并派发 input+change
- `switch_tab` — 切换活动路由到既有 tab（不改 Chrome 可见前景），返回该 tab 简表。
- `type_ref` — 真实按键序列输入（#23）：focus 后逐字符 keyDown(text)+keyUp，
- `wait_idle` — 等 network 静默：从调用时刻起观察 `Network.requestWillBeSent` 与
- `wait_load` — 等页面 load 完成：先宽容地等一次 frameNavigated（导航可能已完成，

