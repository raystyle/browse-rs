# browse-core::cookie_clone

无头引擎登录态按域克隆（#48）：从附着浏览器热迁指定域 cookie 到
临时实例。只读源、绝不写回（用户浏览器零改动，铁律）；全量导出
过宽，按域过滤最小化。

## Functions

- `clone_domains` — 从附着浏览器克隆指定域 cookie 到目标引擎会话。

