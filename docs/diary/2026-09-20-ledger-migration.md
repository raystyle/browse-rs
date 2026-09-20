# 2026-09-20 账本迁移与收口（REQ-013，v0.8.0 加 v0.9.0）

## 账本迁移批（v0.8.0，6b055cd）

- 总台令：ledger.ohmygh.com 全面接管 issue 真源（七仓老账 61 单全量迁入，browse 47 条在册）；旧 issues.ohmygh.com 转只读保役
- 实装：REQ-063 客户端集成（issue 流四命令 + artifact 流 + Ed25519 五头签名道 + JWK 常量 kid 派生 + 私钥双通道）；对读范本 hst_rs v2.6.0
- 评审五轮 F1-F6 全修（含 TEST_ENV_LOCK env 竞态 800 轮压测、publish 三旗标接线、dry-run 承继、回执字段对读 worker、doc 注释族 12 处修复、孤儿投影页清理）CONFIRM 在卷
- 实弹：issue new 201 + artifact publish 201 + attest_dev/attest_prod 双绿（artifact 66857bb2-d1ae-4977-97df-8ab41cc37399，seq 20/21/24）
- 踩坑两记：手抄 artifact_id 双 b 八位首段丢字符 + d1ae 抄成 dae（上游 404 盲诊断三轮，固化 36 字 UUID 形校验）；上报 kid 手算抄错（固化从常量哈希直取，勘误已回正）
- 封版 v0.8.0（tag + 六岗 + r2 播种流水接力）；experience 产物入册（299e9f16，seq 153/154）

## 收口批（v0.9.0，104f37d）

- 总台修正令：全舰队统一标准 crate ledger-client；各仓 CLI 只增不关不删；关闭删除唯一道 omc 工位
- 自研网络签名道移除，薄适配层只留身份面/密档/本地校验/dry-run/截断提示；在册密钥与 kid 不受影响
- v0.1.0 有 URL 拼接舰队级缺陷（总台追注），走 v0.1.1
- 实弹雷：reqwest blocking 在 async main 里嵌套 runtime drop panic（单测全绿、真 GET 才炸）；ledger_call（spawn_blocking）统一适配六网络臂
- 快核 9G 落修：crate 静默默认守卫（0/空串拒假成功）、值域错统一 exit 2、keygen 带 builtinKid 自证、dry-run 签名基七行断言、doc 对齐、REQ-013 与本档补账；终核 F1：实发腿预检补齐（validate_issue_open / validate_artifact_publish 单源，dead-proxy 三探针零网络）
- 版本槽位裁定（用户）：0.9.0（0.x 序列 minor 位即 breaking 位，semver 0.x 惯例；REQ-004 判据的 major 位解读为 1.0 稳定承诺后生效）
- 上游缺口旗总台：BASE_URL 不可覆写（灰度面需要）；attest payload 形漂移（payload.note 旧形与 payload.checks+body 新形并存）
