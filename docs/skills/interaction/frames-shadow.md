# iframe 与 shadow DOM 穿透（#47）

场景：登录挂件、支付 iframe、嵌入式播放器（iframe）；Lit/Web Components 组件库（shadow DOM）。

- 顶层 AX 树（getFullAXTree）不含同源 iframe 内内容与 shadow DOM 内部节点（实弹）：snapshot() 默认只见 Iframe 角色节点
- 穿透用 snapshot({pierce: true})：DOM.getDocument depth -1 pierce true 走 contentDocument 加 shadowRoots，节点带 ref 可直接 clickRef/fillRef
- 跨 frame 坐标已自动提升：clickRef 在节点自身 frame 量中心后沿 frameElement 链累加偏移到顶层视口系（Input 派发系）
- OOPIF（跨域 iframe）pierce 不覆盖：需 Target auto-attach 子 session 路由（session.use 的 frame 版），v1 已知边界
- backendNodeId 是全局的：跨 frame 的 ref 表天然生效，DOM.resolveNode 无需 frame 上下文
