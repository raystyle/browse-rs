---
id: REQ-014
title: 引擎附加旗标直通道（issue #48，clean-chrome 扩展域启用面）
status: implemented
priority: should
trace: 实弹四绿（零 wrapper 起 pinned r4、semanticSnapshot 1 节点、登记后导航 responseReady 双事件、负对照 -32601 与 env 形态同效）；spawn_extra_args_shape 单测锁新形态；评审两轮 F1/G1/G2/G3 全修后 CONFIRM（61d0297，CI ci+docs 双绿）
---

# REQ-014：引擎附加旗标直通道

## Scenario

clean-chrome 的 Browse.* 扩展域须 `--enable-features=CleanChromeBrowseDomain` 开启；spawn 面此前无旗标直通道，托管 pin 形态下用户只能 `--chrome` 指包装脚本绕行（r4 验收窗发现，台账 #48）。

## Criteria

- [x] `--engine-arg <a>` 可叠加与 `BROWSE_ENGINE_ARGS` 空格分隔双通道（UpParams 显式道加 daemon env 道，镜像 proxy #25.5 先例）
- [x] spawn_extra_args 按 argv 元素原样直通（无转义无拆分纪律承袭），单测锁形
- [x] 求值前置透传（browse --engine-arg X '<片段>'）纳入触发条件
- [x] /engine/up 并集序 env 先行、显式随后（chrome 重复旗标后值胜，显式压过 daemon 残留 env）加同值去重（评审 F1，实弹 argv 序双例复验）
- [x] 六处 -32601 指路文案改指正道；与 browse 自身 spawn 旗标撞车慎叠注记（评审 G3）

## Notes

- env 道空格连接对含空格值失真（文案在册），含空格场景走 /engine/up 请求道逐元素直传
- 真跨站换渲染器 wait 不迁移边界在 clean-chrome 双方 diary（clean-chrome c0bbcfd 联动修，r4 起效）
- semver 判据：能力新增取 minor（0.10.0 封版，REQ-004 判据行）
