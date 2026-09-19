# iframe 与 shadow DOM 穿透（#47）

场景：登录挂件、支付 iframe、嵌入式播放器（iframe）；Lit/Web Components 组件库（shadow DOM）。

- 顶层 AX 树（getFullAXTree）不含同源 iframe 内内容与 shadow DOM 内部节点（实弹）：snapshot() 默认只见 Iframe 角色节点
- 穿透用 snapshot({pierce: true})：DOM.getDocument depth -1 pierce true 走 contentDocument 加 shadowRoots，节点带 ref 可直接 clickRef/fillRef
- 跨 frame 坐标已自动提升：clickRef 在节点自身 frame 量中心后沿 frameElement 链累加偏移到顶层视口系（Input 派发系）
- OOPIF（跨域 iframe）：snapshot({pierce:true}) 合并 auto-attach 子 session 的 AX 树（#60），节点带 oopif 标与 frame targetId；子 session 只记 iframe 型 target
- backendNodeId 是全局的：跨 frame 的 ref 表天然生效，DOM.resolveNode 无需 frame 上下文
- findRefs 只搜顶层 AX 树：frame/shadow 内节点搜不到，先 snapshot({pierce:true}) 再按 role/name 挑
- pierce 清单与 AX 投影有结构节点重复（同 backendNodeId 双 ref），name 取 aria-label 或 id 非可见文本
- 被缩放的 iframe（CSS transform scale）坐标提升会静默错位：browse 检测到即报错不猜（去掉 scale 或手点坐标）
