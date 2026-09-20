---
id: REQ-013
title: 账本面（REQ-063 仓级公共账本客户端集成与总台修正令收口）
status: implemented
priority: must
trace: 实弹 issue new 201 + artifact publish/attest_dev/attest_prod 双绿（v0.8.0 批，artifact 66857bb2-d1ae-4977-97df-8ab41cc37399 seq 20/21/24）；收口批 ledger tests 8 件 + v0.1.1 真账本 GET 实弹 + 移除面三拒 exit 2（104f37d）；评审五轮 CONFIRM 在卷
---

# REQ-013：账本面（REQ-063 客户端集成与收口）

## Scenario

issue/artifact 客户端面整体切真源 ledger.ohmygh.com（契约全文 ohmycloud REQ-063）；总台修正令 2026-09-20 收口：全舰队统一标准 crate ledger-client，各仓 CLI 只增不关不删。

## Criteria

- [x] v0.8.0 批（6b055cd）：issue 流四命令加 artifact 流 + Ed25519 五头签名道 + 公钥 JWK 常量（kid=sha256hex(JWK) 按舰队约定）+ 私钥 env/密档双通道零入仓零 argv + artifact_id 36 字 UUID 形校验 + issue new --dry-run 零网络预览（#57 G6 承继）+ TEST_ENV_LOCK 与测试 env 自封闭（评审 F1，800 轮压测 0 失败）
- [x] 收口批（104f37d）：自研网络签名道移除，Cargo 依赖 ledger-client v0.1.1（v0.1.0 有 URL 拼接舰队级缺陷，总台追注）；CLI 只增不关不删：issue close 与 artifact promote 子命令面移除，attest 收三型（attest_dev/attest_prod/verification_failed）；关闭与删除唯一道 = omc 工位经 herdr 委托
- [x] 薄适配层只留：身份面（PUBKEY_JWK 与 kid 派生，keygen 带 builtinKid 自证对账）、密档管理（base64url seed，在册密钥不受收口影响）、本地校验（exit 2 用法错口径）、dry-run、#52 家族截断提示
- [x] reqwest blocking 在 async main 的嵌套 runtime drop 雷以 ledger_call（spawn_blocking）统一适配
- [x] crate 静默默认守卫：issue_new 回 0 / artifact_publish 回空串即报错（评审 G4）

## Notes

- 参数面随标准 crate 收窄：publish 的 --summary/--outcome/--git-sha 撤（标准面 --note）；上游缺口已旗总台：BASE_URL 不可覆写（灰度/本地捕获测试面需要）、attest payload 形漂移（payload.note 旧形 vs payload.checks+body 新形并存）
- 旧 REQ-057 issues.ohmygh.com 客户端通道已摘（零调用实证）；旧服务只读保役
