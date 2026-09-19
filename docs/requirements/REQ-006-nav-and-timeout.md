---
id: REQ-006
title: 批 1 时序口径与导航族（issue #51/#19/#39）
status: implemented
priority: must
trace: timeout_tests 三件单测（js_host）加 e2e 批 1 块（goto/历史/reload/check 幂等/submit/秒口径双态）加 surface 六新条目；issue #51/#19/#39 验收正文中九断言对齐
---

# REQ-006：批 1 时序口径与导航族（issue #51/#19/#39）

## Scenario

agent 驾驶页面时：wait 类 timeout 单位混用（helper 收毫秒、BROWSE_EVAL_TIMEOUT 是秒）埋 ms/s 误写雷；导航组合要手写 Page.navigate 加 waitLoad 两步且 loadEventFired 竞态假超时要自己踩；历史导航（回退/前进/刷新）与防呆小动词（check/uncheck/fill 提交）只能裸调 CDP。

## Criteria

对应 issue #51、#19、#39（台账真源，验收正文以 issue 为准，此处只列锚点）：

- [ ] #51 timeout 统一为秒并在 --llms 与 surface 明示；wait 类参数加混用告警（大于 3600 视为毫秒误写，告警并按 ms/1000 解释或封顶 600 秒）；BROWSE_EVAL_TIMEOUT 与片段内 timeout 口径一致
- [ ] #51 回归锁：waitLoad(15) 等价旧 15000ms；waitLoad(15000) 触发混用告警并按秒口径处理
- [ ] #19 goto(url, opts?) 宿主函数：navigate 加 waitLoad 一体，可选 waitIdle 网络静默，返回 url/title/elapsedMs；已加载页立即返回不阻塞；timeout 参数按秒口径
- [ ] #19 clickRef(ref, {waitNav: true})：链接型点击后自动等导航稳定
- [ ] #19 手册在 waitFor 词条标注 loadEventFired 竞速风险，推荐 waitLoad/goto 替代
- [ ] #39 goBack(delta?)、goForward(delta?)、reload({ignoreCache})；checkRef、uncheckRef；fillRef/fillInput 支持 opts.submit 顺带 Enter
- [ ] surface 目录与 aidoc 投影随卷；e2e 按需补真引擎回归

实现后回填 frontmatter 的 trace（测试路径或验收命令），状态改 implemented。
