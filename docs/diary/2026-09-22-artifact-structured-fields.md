# 2026-09-22 artifact 结构化字段批（总台令：三件在册产物补 summary/outcome）

背景：总台转用户口径，ledger 人类网页已按 JSON 字段渲染（技术描述加 outcome 加验收判据直出），但本仓三件在册 artifact（lesson 6768e7e0、experience 299e9f16、prototype 66857bb2）publish 载荷 summary 与 outcome 均空，构造性缺陷：三件都是 ledger-client v0.1.1 客户端发的，该版 artifact_publish 无 summary/outcome 形参（104f37d 收口时因标准 crate 面窄还撤过这三个 CLI 旗标）。API 详情读面核证三件全空（summary/outcome/git_range/version 均 None）。

## 修法（升 dep 复旗标，实发三件 supersede）

- ledger-client dep v0.1.1 升 v0.1.3（artifact_publish_full 十参：summary/outcome/git_sha 结构化字段，非正文拼接）
- CLI 复三旗标：`--summary`（一行技术摘要）、`--outcome`（success|failure 枚举，非法值网络前 exit 2）、`--git-sha`（提交锚落表）；Mode 与 surface 词条随迁，arg_contract 补 outcome 枚举锁
- 实发三件同 name supersede（账本只增语义）加 attest_dev：prototype 5b160f36（outcome success，git_range 8dd920a..6b055cd，git_sha 6b055cd）、experience eef7b8e4（outcome success，同区间加 hst_rs 对读 dep）、lesson 559e873f（outcome failure，git_range bbc7bc2..104f37d，git_sha 104f37d；summary=阻塞根因加复现边界加解法全文，digest 即其 sha256）
- 回读核证：publish 载荷 outcome/summary/deps/git_range 全在（timeline payload 逐字段）；attest_dev 落 seq 180-183；详情投影列不抬 summary 属服务端表投影形态，网页按 payload 渲染（总台口径）

## 封版与流程

能力新增（publish 参数面扩展）取 minor，v0.14.0 随批。评审、推送、发版随后补账。
