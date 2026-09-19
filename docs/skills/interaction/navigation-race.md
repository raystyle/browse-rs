# 导航竞速与等待判官

场景：navigate 之后立刻取内容/截图/等事件，拿到旧文档或假超时。

- 等加载用 `goto(url, opts)` 或 `waitLoad(s)`（readyState 轮询，无竞速窗）；不要 `waitFor("Page.loadEventFired")`：事件在你注册前已发就假超时（#19）
- 等导航事件用 `Page.frameNavigated`，不用 loadEventFired
- 同片段 navigate 后紧接 screenshot 的提交窗竞速由 host 层统一等提交（#34）；reload/历史跳/点击导航走有界提交等待（wait_settled，grace 窗后无导航迹象即返回）
- 同文档 fragment 导航不换代：ref 仍有效，代际不递增（#57 F3）
- timeout 一律秒口径；不小于 1000 的值按毫秒误写换算并告警（#51/#57 F1）
- Chrome 禁止顶级跳转 data: URL：测试里的链接目标用 routeMock 假域（批 2 实弹坑）
