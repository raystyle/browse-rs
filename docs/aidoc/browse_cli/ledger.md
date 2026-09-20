# browse-cli::ledger

账本面（REQ-063 仓级公共账本客户端，Phase 3 CLI 原生集成）：
issue 流（bug 加 improvement 任务的立项、事件与关单）加 artifact 流
（产物共享库 publish 加 attest 加 promote）。真源 = ledger.ohmygh.com
（替代 issues.ohmygh.com 客户端面；旧服务过渡保役，REQ-063 切换策）。
写入五头签名道（Ed25519）：私钥运行时从 env `BROWSE_LEDGER_PRIVATE_KEY`
（base64url seed）或本地密档 `~/.browse-rs/ledger/ed25519.key` 读（不进仓不
进 argv）；公钥 JWK 常量内置（身份分发面，总台在册）。GET 不签名不
受配额；写入 per-key 50 条每 UTC 日，幂等命中不耗（服务端契约）。

## Functions

- `artifact_attest` — artifact 事件（POST /repos/`<id>`/artifacts/`<id>`/attestations）：type 见
- `artifact_publish` — artifact 发布（POST /repos/`<id>`/artifacts）：digest = 正文或记录哈希
- `artifacts_list` — artifact 列表（GET）：current=1 投影当前有效集，env 过滤 dev|prod。
- `clamp_issue_limit` — issue list 的 limit 钳制（1 至 100，服务端上限；#52 家族标准）：钳制
- `issue_close` — issue 关单链（result 引 digest 先行，status to=done 收尾；REQ-063 关
- `issue_event` — issue 事件（POST /repos/`<id>`/issues/`<n>`/events）：type 见
- `issue_list_saturated` — issue list 饱和判定（#52 家族标准）：返回条数不少于钳制后 limit 即示
- `issue_list_truncation_hint` — issue list 饱和提示行（#52/#53 家族标准）：返回条数打满钳制后 limit
- `issue_open` — issue 开单（POST /repos/`<id>`/issues）：title trim 后 1 至 200，kind 缺
- `issue_open_dry_run` — issue 开单的 dry-run（#57 G6 评审强制项，账本面保留）：本地同规校验加
- `issue_show` — issue 详情（GET /repos/`<id>`/issues/`<n>`）：projection 加 timeline。
- `issues_list` — issue 列表（GET，家族形）：limit 缺省 100（钳制 1 至 100），before
- `key_id` — kid = sha256hex(规范化 JWK {crv,kty,x}，键序字母、紧凑无空白)；常量
- `keygen_write` — 密钥对生成（一次性或轮换）：写私钥密档（0600，目录 `~/.browse-rs/ledger/`）
- `ledger_base` — 账本基址（env `BROWSE_LEDGER_URL` 覆盖，测与灰度）。
- `load_signing_key` — 载入签名私钥：env `BROWSE_LEDGER_PRIVATE_KEY`（base64url seed）优先，
- `pairing_ok` — 本地私钥与内置公钥 JWK 的配对自检（评审 G3）：密档/env 缺位回 None
- `private_key_path` — 私钥密档路径（`~/.browse-rs/ledger/ed25519.key`，内容 = base64url 32 字节
- `signature_base` — 签名基构造（服务端 signatureBase 同源）：`v1\nPOST\n<路径>\n<ts>\n
- `validate_artifact_id` — artifact_id 形校验（总台建议 2026-09-20）：36 字 UUID 形
- `validate_digest` — digest 校验（服务端 DIGEST_RE 同源）：`sha256:<64hex 小写>` 形合规即 Ok。

## Constants

- `ARTIFACT_KINDS` — artifact kind 集（服务端 ARTIFACT_KINDS 同源）。
- `ATTEST_TYPES` — attestation 类型集（服务端 ATTEST_TYPES 同源）。
- `ISSUE_EVENT_TYPES` — issue 事件类型集（服务端 ISSUE_EVENT_TYPES 同源）。
- `ISSUE_KINDS` — issue kind 集（服务端 ISSUE_KINDS 同源）：bug（BUG 错误任务）与
- `ISSUE_STATUSES` — issue 状态集（服务端 ISSUE_STATUSES 同源；done 须先有 result 事件）。
- `PUBKEY_JWK` — 本仓公钥 JWK（REQ-063 裁 2：常量集成进 CLI；字母键序紧凑形，kid 即
- `REPO_ID` — 本仓账本身份（REQ-063：repo_id = 规范化 remote）。

