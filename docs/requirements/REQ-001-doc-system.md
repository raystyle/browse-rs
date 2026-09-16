---
id: REQ-001
title: 建立文档体系
status: implemented
priority: must
trace: uv run /mnt/d/ProjectEvo/plugins/project-evo/skills/dev-evo/scripts/check.py . 退出码 0
---

# REQ-001：建立文档体系

## Scenario

维护者与编码 agent 协作需要一个可门禁的文档体系：需求先登记再实现、不可逆决策留 why、过程留痕可追溯，且骨架合规状态可用一条命令检验（dev-evo 权威标准）。

## Criteria

- [x] docs 五目录在位：adr、requirements、guides、diary、research
- [x] requirements 带 README 索引与 0000 模板，首个 REQ 登记体系本身
- [x] diary 记初始化当日首笔；research 无真实研究暂空（SKIP 合法）
- [x] docs/README.md 文档地图：活跃体系表带红线注记，aidoc/surface/architecture.md 以投影与专档登记
- [x] AGENTS 五节合同齐备，Commands 在册 fmt/clippy/test 与 aidoc/surface/check 三门禁
- [x] check.py 诊断退出码 0（手写件禁字源头清零；aidoc 条目分隔符是工具渲染格式，走登记豁免 `^docs/aidoc/`）
