# browse-cli::ledger

 账本薄适配层（REQ-063；总台修正令 2026-09-20 收口）：签名道与只增面
 全在 ledger-client crate（github.com/raystyle/ledger-rs v0.1.1，全舰队
 唯一实现；v0.1.0 有 URL 拼接舰队级缺陷已避），本层只留本仓身份面
（公钥 JWK 常量与 kid 派生）、密档管理
 （base64url seed，env `BROWSE_LEDGER_PRIVATE_KEY` 或本地密档双通道）、
 命令面本地校验、`--dry-run` 载荷预览与 #52 家族截断提示。CLI 只增不关
 不删：issue close 与 artifact promote/demote/supersede 面已移除，关闭
 与删除唯一道 = 开发工作台经 herdr 委托 omc 工位执行（`omc ledger issue
 status <repo> <n> <to>` 与 `omc ledger issue delete`）。真源 =
 ledger.ohmygh.com（替代 issues.ohmygh.com 客户端面；旧服务只读保役）。

## Functions

- `clamp_issue_limit` — issue list 的 limit 钳制（1 至 100，服务端上限；#52 家族标准）：钳制
- `client` — 组装标准客户端（签名道与只增面在 ledger-client；本仓身份面注入）。
- `issue_list_saturated` — issue list 饱和判定（#52 家族标准）：返回条数不少于钳制后 limit 即示
- `issue_list_truncation_hint` — issue list 饱和提示行（#52/#53 家族标准）：返回条数打满钳制后 limit
- `issue_open_dry_run` — issue 开单的 dry-run（#57 G6 评审强制项，账本面保留）：本地同规校验加
- `key_id` — kid = sha256hex(规范化 JWK {crv,kty,x}，键序字母、紧凑无空白)；常量
- `keygen_write` — 密钥对生成（一次性或轮换）：写私钥密档（0600）并返回 (kid, JWK, 旧
- `load_keypair` — 载入账本身份：env `BROWSE_LEDGER_PRIVATE_KEY`（base64url seed）优先，
- `pairing_ok` — 本地私钥与内置公钥 JWK 的配对自检（评审 G3）：密档/env 缺位回 None
- `private_key_path` — 私钥密档路径（`~/.browse-rs/ledger/ed25519.key`，内容 = base64url 32 字节
- `validate_artifact_id` — artifact_id 形校验（总台建议 2026-09-20）：36 字 UUID 形
- `validate_digest` — digest 校验（服务端 DIGEST_RE 同源）：`sha256:<64hex 小写>` 形合规即 Ok。

## Constants

- `ISSUE_KINDS` — issue kind 集（服务端 ISSUE_KINDS 同源，本地预检省一次网络往返）：bug
- `PUBKEY_JWK` — 本仓公钥 JWK（REQ-063 裁 2：常量集成进 CLI；字母键序紧凑形，kid 即
- `REPO_ID` — 本仓账本身份（REQ-063：repo_id = 规范化 remote）。

