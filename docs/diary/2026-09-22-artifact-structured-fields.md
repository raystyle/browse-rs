# 2026-09-22 artifact 结构化字段批（总台令：三件在册产物补 summary/outcome）

背景：总台转用户口径，ledger 人类网页已按 JSON 字段渲染（技术描述加 outcome 加验收判据直出），但本仓三件在册 artifact（lesson 6768e7e0、experience 299e9f16、prototype 66857bb2）publish 载荷 summary 与 outcome 均空，构造性缺陷：三件都是 ledger-client v0.1.1 客户端发的，该版 artifact_publish 无 summary/outcome 形参（104f37d 收口时因标准 crate 面窄还撤过这三个 CLI 旗标）。API 详情读面核证三件全空（summary/outcome/git_range/version 均 None）。

## 修法（升 dep 复旗标，实发三件 supersede）

- ledger-client dep v0.1.1 升 v0.1.3（artifact_publish_full 十参：summary/outcome/git_sha 结构化字段，非正文拼接）
- CLI 复三旗标：`--summary`（一行技术摘要）、`--outcome`（success|failure 枚举，非法值网络前 exit 2）、`--git-sha`（提交锚落表）；Mode 与 surface 词条随迁，arg_contract 补 outcome 枚举锁
- 实发三件同 name supersede（账本只增语义）加 attest_dev：prototype 5b160f36（outcome success，git_range 8dd920a..6b055cd，git_sha 6b055cd）、experience eef7b8e4（outcome success，同区间加 hst_rs 对读 dep）、lesson 559e873f（outcome failure，git_range bbc7bc2..104f37d，git_sha 104f37d；summary=阻塞根因加复现边界加解法全文，digest 即其 sha256）
- 回读核证：publish 载荷 outcome/summary/deps/git_range 全在（timeline payload 逐字段）；attest_dev 落 seq 180-183；详情投影列不抬 summary 属服务端表投影形态，网页按 payload 渲染（总台口径）

## 封版与流程（同日续）

- 评审一轮 CONFIRM（(a) 十参接线、(b) 枚举拦截、(c) dep v0.1.3 兼容面含 Cargo.lock 锚 8d0a599）加 G1 随批（summary 非空限长与 git_sha 十六进制形本地预检）；f12ba81 一次管道吞红事故（clippy collapsible-if 被尾管吃掉推了出去，CI docs 流红），5516b96 抢修后 CI 双绿（ci 35679871180、docs 35679871184）
- 封版 v0.14.0（能力新增取 minor）；release.ps1 全链 exit 0；seed run 35680140972 success 1m3s；stable/latest 滚 0.14.0
- 五端拉平（镜像原生链）：wsl、lan-win、lan-mac（ark 委托）、lan-ubuntu、lan-linux 全 0.14.0
- 回执总台：三件新 artifact_id 与 digest 见修法节，timeline payload 逐字段在册
