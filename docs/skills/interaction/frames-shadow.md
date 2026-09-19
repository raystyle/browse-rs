# iframe 与 shadow DOM 穿透（#47）

场景：登录挂件、支付 iframe、嵌入式播放器（iframe）；Lit/Web Components 组件库（shadow DOM）。

- 顶层 AX 树（getFullAXTree）不含同源 iframe 内内容与 shadow DOM 内部节点（实弹）：snapshot() 默认只见 Iframe 角色节点
- 穿透用 snapshot({pierce: true})：DOM.getDocument depth -1 pierce true 走 contentDocument 加 shadowRoots，节点带 ref 可直接 clickRef/fillRef
- 跨 frame 坐标已自动提升：clickRef 在节点自身 frame 量中心后沿 frameElement 链累加偏移到顶层视口系（Input 派发系）
- OOPIF（跨域 iframe）：snapshot({pierce:true}) 合并子 session 的 AX 树（#60），节点带 oopif 标、frame targetId 与 ownerSession；**子 session 靠「发现 + 显式 attach」拿**：route 里 Target.targetCreated 只记 iframe 型，`sync_child_sessions`（随 tab 入口与 pierce 调用）扫 Target.getTargets 逐个 attachToTarget(flatten)，OOPIF 销毁按 Target.detachedFromTarget 摘账、重连随 reset 清账
- OOPIF 上的 ref 全动词可用：clickRef/hoverRef/dblclickRef/dragRef 在子 session 量中心后用父页 iframe 元素的 rect 提升到顶层视口系（遮挡检查在父页对 iframe 做一次）；fillRef/typeRef/selectOption/checkRef 的焦点与键盘走子 session；highlight 注入到子 frame 自己文档；screenshot({ref}) 的裁剪同样提升。**嵌套 OOPIF（OOPIF 里再套 OOPIF）只提升一层**，属 v1 边界
- 本机 Chrome 152 实测：Target.setAutoAttach 无论挂浏览器级还是页面级、带不带 filter:[{type:"iframe"}]，都不产生 iframe 型 attachedToTarget（只来 page/browser_ui/service_worker），所以 OOPIF 不能靠 auto-attach，必须显式 attach
- backendNodeId 是全局的：跨 frame 的 ref 表天然生效，DOM.resolveNode 无需 frame 上下文
- findRefs 只搜顶层 AX 树：frame/shadow 内节点搜不到，先 snapshot({pierce:true}) 再按 role/name 挑
- pierce 清单与 AX 投影有结构节点重复（同 backendNodeId 双 ref），name 取 aria-label 或 id 非可见文本
- 被缩放的 iframe（CSS transform scale）坐标提升会静默错位：browse 检测到即报错不猜（去掉 scale 或手点坐标）
