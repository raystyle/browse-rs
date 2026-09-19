---
id: REQ-007
title: 批 2 token 经济与可观测三件（issue #36/#37/#49）
status: implemented
priority: must
trace: e2e 批 2 块（findRefs 命中带可点 ref、depth 降节点、console 分级、jsErrors、requests/detail 对账、detect ok/blank/login-wall 三判）加真页实弹（example.com：snapshot 8 节点、depth2 7、findRefs Learn more 命中 2、requests 200、detect ok）
---

# REQ-007：批 2 token 经济与可观测三件（issue #36/#37/#49）

## Scenario

agent 在大页面上拿结构靠全量 snapshot（X 首页 379 节点）；排障要手开 Runtime/Network 域加 peekEvents 拼装；页面打不开/登不上/白屏只能瞎猜。三单分别补：捕获与检索分离、可观测三件、页面态判官。

## Criteria

对应 issue #36、#37、#49（台账真源，验收正文以 issue 为准，此处只列锚点）：

- [x] #36 snapshot({ref, depth}) 参数化（单元素子树部分展开、限深浅扫）加 childIds 随卷输出
- [x] #36 findRefs(query, {insensitive, context})：服务端匹配只回命中加祖先链，节点带 ref 可直接 clickRef
- [x] #37 console({since, minLevel}) 分级；jsErrors({since})；requests({since, filter})；requestDetail(indexOrRequestId)
- [x] #37 Runtime/Network 域随 tab 入口自动开（ensure_page_enabled 扩三域）
- [x] #49 detect() 七判 {verdict, evidence[], suggestion}：challenged/rate-limited/blocked/stalled/login-wall/blank/loading/ok
- [x] e2e 按 issue 测试用例落锁：findRefs 命中、depth 节点数下降、console 分级、jsErrors 取未捕获、requests/detail 对账、detect 多判
- [x] surface 目录与 aidoc 投影随卷

实现后回填 frontmatter 的 trace，状态改 implemented。
