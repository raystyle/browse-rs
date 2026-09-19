# 鼠标与输入原语

场景：hover 菜单、右键、双击、滚轮、拖拽、按住键。

- hover：`hoverRef(ref)` / `hoverAt(x, y)`（mouseMoved 路径触发 CSS :hover）
- 键与次数：`clickAt(x, y, {button, clickCount})`、`clickRef(ref, {button, clickCount, waitNav})`；button 五键白名单（left/right/middle/back/forward），非法值当场报错
- 滚轮：`mouseWheel(dx, dy)` 走手势合成（因）dispatchMouseEvent 的 mouseWheel 导航后首发被渲染器吞（监听注册握手），第二发起才触发（#35 实测定谳）；要精确单事件裸调并自发两次
- 拖拽：`mouseDown` 到 `mouseMove` 到 `mouseUp` 手拼（坐标缺省沿用最近落点，按下前自动 move 打点）
- 后台 tab 的 Input 可能挂起（30s）：先 `Target.activateTarget` 或接受人机共存不抢前台
- findRefs 按文本找节点会同时命中容器与其 StaticText 子节点：交互目标优先按 role 挑（button/checkbox），别拿文本节点点击
