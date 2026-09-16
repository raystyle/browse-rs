# Requirements 索引

> 需求登记：新需求先立 REQ 再实现，实现后回填 trace（测试路径或验收命令）。新建拷 0000-template.md，编号接当前最大号。状态 draft 到 implemented 到 rejected。

| id | 状态 | 优先级 | 标题 | trace |
|---|---|---|---|---|
| REQ-001 | implemented | must | 建立文档体系 | code-kit check.py 退出码 0 |
| REQ-002 | implemented | should | browse --llms 发现通道 | browse --llms [--full\|--json] 三形态冒烟 + surface_contract |
| REQ-003 | draft | must | browse 托管 clean-chrome（部署升级维护与自有用户数据） | null |
| REQ-004 | implemented | must | semver 判据与封版流（0.1.0 首封） | tag v0.1.0 + 三路门禁（diary 2026-09-16 封版节） |
