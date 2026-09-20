//! 账本面（REQ-063 仓级公共账本客户端，Phase 3 CLI 原生集成）：
//! issue 流（bug 加 improvement 任务的立项、事件与关单）加 artifact 流
//! （产物共享库 publish 加 attest 加 promote）。真源 = ledger.ohmygh.com
//! （替代 issues.ohmygh.com 客户端面；旧服务过渡保役，REQ-063 切换策）。
//! 写入五头签名道（Ed25519）：私钥运行时从 env `BROWSE_LEDGER_PRIVATE_KEY`
//! （base64url seed）或本地密档 `~/.browse-rs/ledger/ed25519.key` 读（不进仓不
//! 进 argv）；公钥 JWK 常量内置（身份分发面，总台在册）。GET 不签名不
//! 受配额；写入 per-key 50 条每 UTC 日，幂等命中不耗（服务端契约）。

use std::path::PathBuf;

use base64::Engine;
use ed25519_dalek::Signer;
use ed25519_dalek::SigningKey;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// 本仓账本身份（REQ-063：repo_id = 规范化 remote）。
pub const REPO_ID: &str = "github.com/raystyle/browse-rs";

/// ledger 密档根（`~/.browse-rs/ledger/`，BROWSE_NAME 命名实例各一份）。
fn ledger_home() -> Result<std::path::PathBuf, String> {
    Ok(browse_core::paths::state_dir().join("ledger"))
}

/// 本仓公钥 JWK（REQ-063 裁 2：常量集成进 CLI；字母键序紧凑形，kid 即
/// 其 sha256hex）。私钥恒在提交侧（env 或本地密档），不进仓。
pub const PUBKEY_JWK: &str =
    "{\"crv\":\"Ed25519\",\"kty\":\"OKP\",\"x\":\"vGpYNa-iVpc3k00mG3aPZbk94NlHeWEABOqqal65Pag\"}";

/// issue kind 集（服务端 ISSUE_KINDS 同源）：bug（BUG 错误任务）与
/// improvement（改进优化任务），缺省 bug。
pub const ISSUE_KINDS: &[&str] = &["bug", "improvement"];

/// issue 事件类型集（服务端 ISSUE_EVENT_TYPES 同源）。
pub const ISSUE_EVENT_TYPES: &[&str] = &[
    "claim",
    "release",
    "status",
    "result",
    "blocker",
    "supersede",
];

/// issue 状态集（服务端 ISSUE_STATUSES 同源；done 须先有 result 事件）。
pub const ISSUE_STATUSES: &[&str] = &["open", "claimed", "in_progress", "blocked", "done"];

/// artifact kind 集（服务端 ARTIFACT_KINDS 同源）。
pub const ARTIFACT_KINDS: &[&str] = &[
    "binary",
    "image",
    "wasm",
    "sbom",
    "schema",
    "openapi",
    "eval-set",
    "benchmark",
    "runbook",
    "decision",
    "attested-report",
    "experience",
    "lesson",
    "research",
    "prototype",
];

/// attestation 类型集（服务端 ATTEST_TYPES 同源）。
pub const ATTEST_TYPES: &[&str] = &[
    "attest_dev",
    "attest_prod",
    "verification_failed",
    "promote",
    "demote",
    "supersede",
];

/// 账本基址（env `BROWSE_LEDGER_URL` 覆盖，测与灰度）。
pub fn ledger_base() -> String {
    std::env::var("BROWSE_LEDGER_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "https://ledger.ohmygh.com".into())
}

/// kid = sha256hex(规范化 JWK {crv,kty,x}，键序字母、紧凑无空白)；常量
/// 本身已是该形，直接哈希（单一真相，不在代码里另存 kid 字面量）。
pub fn key_id() -> String {
    let mut h = Sha256::new();
    h.update(PUBKEY_JWK.as_bytes());
    format!("{:x}", h.finalize())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

fn unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 私钥密档路径（`~/.browse-rs/ledger/ed25519.key`，内容 = base64url 32 字节
/// seed；0600 由 keygen 落）。
///
/// # Errors
///
/// 密档根解析失败。
pub fn private_key_path() -> Result<PathBuf, String> {
    Ok(crate::ledger::ledger_home()?.join("ed25519.key"))
}

/// 签名基构造（服务端 signatureBase 同源）：`v1\nPOST\n<路径>\n<ts>\n
/// <nonce>\n<idem>\n<sha256hex(body)>` 各行换行连。纯函数可单测。
pub fn signature_base(
    method: &str,
    path: &str,
    ts: &str,
    nonce: &str,
    idem: &str,
    body: &str,
) -> String {
    [
        "v1",
        method,
        path,
        ts,
        nonce,
        idem,
        &sha256_hex(body.as_bytes()),
    ]
    .join("\n")
}

fn b64url(seed: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(seed)
}

/// 密钥对生成（一次性或轮换）：写私钥密档（0600，目录 `~/.browse-rs/ledger/`）
/// 并返回 (kid, JWK, 旧 kid)；私钥不打印不进 argv，公钥 JWK 供总台在册
/// （在册后方可写入）。在位密档非 `--force` 即拒（防静默销毁）。
/// # Errors
///
/// 失败返回 `String` 错误（路径与写入类）。
pub fn keygen_write(force: bool) -> Result<(String, String, Option<String>), String> {
    let path = private_key_path()?;
    // 覆盖守卫（评审 G2b）：在册私钥静默销毁 = 后续写入全 401 且旧钥不可
    // 恢复；非 force 即拒，force 时先带出旧 kid 供对账。
    let old_kid = if path.exists() {
        let old = load_signing_key().ok().map(|k| {
            let x = b64url(&k.verifying_key().to_bytes());
            sha256_hex(format!("{{\"crv\":\"Ed25519\",\"kty\":\"OKP\",\"x\":\"{x}\"}}").as_bytes())
        });
        if !force {
            return Err(format!(
                "密档已在位（{}）；重复生成会销毁在册私钥，确认轮换加 --force",
                path.display()
            ));
        }
        old
    } else {
        None
    };
    let sk = SigningKey::generate(&mut rand::rngs::OsRng);
    let x = b64url(&sk.verifying_key().to_bytes());
    let jwk = format!("{{\"crv\":\"Ed25519\",\"kty\":\"OKP\",\"x\":\"{x}\"}}");
    let kid = sha256_hex(jwk.as_bytes());
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    // 原子 0600（评审 G2a）：OpenOptions 带模式一次落，消两步间的 0644 窗。
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true).mode(0o600);
        let mut f = opts
            .open(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        use std::io::Write;
        f.write_all(b64url(&sk.to_bytes()).as_bytes())
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    #[cfg(not(unix))]
    std::fs::write(&path, b64url(&sk.to_bytes()))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((kid, jwk, old_kid))
}

/// 本地私钥与内置公钥 JWK 的配对自检（评审 G3）：密档/env 缺位回 None
/// （无法判），在位回配对与否——不配对则写入会全体 401 且难归因。
///
/// # Errors
///
/// 内置 JWK 常量解析失败（形变即测试红，运行时不可达）。
pub fn pairing_ok() -> Result<Option<bool>, String> {
    let key = match load_signing_key() {
        Ok(k) => k,
        Err(_) => return Ok(None),
    };
    let x = b64url(&key.verifying_key().to_bytes());
    let expect = serde_json::from_str::<Value>(PUBKEY_JWK)
        .ok()
        .and_then(|j| j["x"].as_str().map(String::from));
    Ok(expect.map(|e| e == x))
}

/// 载入签名私钥：env `BROWSE_LEDGER_PRIVATE_KEY`（base64url seed）优先，
/// 次本地密档。不进 argv 不进仓。
///
/// # Errors
///
/// env 值或密档内容非 base64url 32 字节 seed，或密档不可读。
pub fn load_signing_key() -> Result<SigningKey, String> {
    use ed25519_dalek::SecretKey;
    let seed_b64 = std::env::var("BROWSE_LEDGER_PRIVATE_KEY")
        .ok()
        .filter(|s| !s.trim().is_empty());
    let seed = match seed_b64 {
        Some(s) => base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(s.trim().as_bytes())
            .map_err(|e| format!("BROWSE_LEDGER_PRIVATE_KEY 非 base64url：{e}"))?,
        None => {
            let p = private_key_path()?;
            let text = std::fs::read_to_string(&p).map_err(|e| {
                format!(
                    "私钥密档不可读（{}；或设 env BROWSE_LEDGER_PRIVATE_KEY）：{e}",
                    p.display()
                )
            })?;
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(text.trim().as_bytes())
                .map_err(|e| format!("密档内容非 base64url（{}）：{e}", p.display()))?
        }
    };
    let arr: [u8; 32] = seed
        .as_slice()
        .try_into()
        .map_err(|_| "seed 须 32 字节（Ed25519）".to_string())?;
    Ok(SigningKey::from_bytes(&SecretKey::from(arr)))
}

fn rand_hex32() -> String {
    use rand::RngCore;
    let mut b = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// 写入道（POST 全必填五头）：构造头、签名、发请求、解回执。非 2xx 透
/// 传服务端 error；幂等命中（200 加 replay）原样透出由调用方标注。
fn post_signed(path: &str, body: &Value) -> Result<Value, String> {
    post_signed_with_idem(path, body, None)
}

/// [`post_signed`] 的确定性幂等键形（评审 G1）：同键同内容重试被服务端
/// 幂等命中返 replay（不重复追加事件），关单链用。
fn post_signed_with_idem(
    path: &str,
    body: &Value,
    idem_fixed: Option<&str>,
) -> Result<Value, String> {
    let key = load_signing_key()?;
    let body_text = serde_json::to_string(body).map_err(|e| format!("serialize body: {e}"))?;
    let ts = unix_secs().to_string();
    let nonce = rand_hex32();
    let idem = idem_fixed.map(String::from).unwrap_or_else(rand_hex32);
    let base = signature_base("POST", path, &ts, &nonce, &idem, &body_text);
    let sig = key.sign(base.as_bytes()).to_bytes();
    let url = format!("{}{path}", ledger_base());
    let resp = ureq::post(&url)
        .set("User-Agent", "browse-ledger")
        .set("Content-Type", "application/json")
        .set("Idempotency-Key", &idem)
        .set("X-Key-Id", &key_id())
        .set("X-Timestamp", &ts)
        .set("X-Nonce", &nonce)
        .set("X-Signature", &b64url(&sig))
        .timeout(std::time::Duration::from_secs(20))
        .send_string(&body_text);
    let (status, text) = match resp {
        Ok(r) => (
            r.status(),
            r.into_string().map_err(|e| format!("read body: {e}"))?,
        ),
        Err(ureq::Error::Status(code, r)) => (code, r.into_string().unwrap_or_default()),
        Err(e) => return Err(format!("post {url}: {e}")),
    };
    let v: Value = serde_json::from_str(&text).unwrap_or(json!({}));
    if (200..300).contains(&status) && v["ok"] == json!(true) {
        Ok(v)
    } else {
        Err(format!(
            "ledger status={status} error={}",
            v["error"]
                .as_str()
                .unwrap_or(&text.chars().take(200).collect::<String>())
        ))
    }
}

fn get_json(path: &str) -> Result<Value, String> {
    let url = format!("{}{path}", ledger_base());
    let resp = ureq::get(&url)
        .set("User-Agent", "browse-ledger")
        .timeout(std::time::Duration::from_secs(20))
        .call();
    let (status, text) = match resp {
        Ok(r) => (
            r.status(),
            r.into_string().map_err(|e| format!("read body: {e}"))?,
        ),
        Err(ureq::Error::Status(code, r)) => (code, r.into_string().unwrap_or_default()),
        Err(e) => return Err(format!("get {url}: {e}")),
    };
    let v: Value = serde_json::from_str(&text).unwrap_or(json!({}));
    if status == 200 && v["ok"] == json!(true) {
        Ok(v)
    } else {
        Err(format!(
            "ledger status={status} error={}",
            v["error"]
                .as_str()
                .unwrap_or(&text.chars().take(200).collect::<String>())
        ))
    }
}

/// issue list 的 limit 钳制（1 至 100，服务端上限；#52 家族标准）：钳制
/// 单源，命令面默认值与饱和提示判定共用。
pub fn clamp_issue_limit(limit: u32) -> u32 {
    limit.clamp(1, 100)
}

/// issue list 饱和判定（#52 家族标准）：返回条数不少于钳制后 limit 即示
/// 警；用 `>=` 不用 `==`，服务端若返回多于请求值，`==` 会静默漏报。
pub fn issue_list_saturated(returned: usize, eff: u32) -> bool {
    returned >= eff as usize
}

/// issue list 饱和提示行（#52/#53 家族标准）：返回条数打满钳制后 limit
///（或 before 翻页面 has_more 为真）时出此行到 stderr，指向 `--status`
/// 不适用（账本列表无状态过滤）故给 `--limit` 提高、`--before` 翻更旧
/// 一页与账本网页面；不饱和不出。
pub fn issue_list_truncation_hint(eff: u32) -> String {
    format!(
        "issue.list.truncated=limit-reached limit={eff} hint=返回条数打满 limit，可能仍有更多；提高 --limit（上限 100）、--before <id> 翻更旧一页，或账本网页面看全量 {}/repos/{REPO_ID}/issues",
        ledger_base()
    )
}

/// artifact_id 形校验（总台建议 2026-09-20）：36 字 UUID 形
/// `8-4-4-4-12` 小写 hex 连四杠；截断或转写错本地即报，免上游 404 往返。
///
/// # Errors
///
/// 非 36 字 UUID 形（段长、非法字符、分隔位）。
pub fn validate_artifact_id(s: &str) -> Result<(), String> {
    let ok = s.len() == 36
        && s.as_bytes()[8] == b'-'
        && s.as_bytes()[13] == b'-'
        && s.as_bytes()[18] == b'-'
        && s.as_bytes()[23] == b'-'
        && s.bytes().enumerate().all(|(i, c)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                true
            } else {
                c.is_ascii_digit() || matches!(c, b'a'..=b'f')
            }
        });
    if ok {
        Ok(())
    } else {
        Err(format!(
            "artifact_id 须 36 字 UUID 形（8-4-4-4-12 小写 hex），得 {s}（len={}）；下一步：browse artifact list 取真值",
            s.len()
        ))
    }
}

/// digest 校验（服务端 DIGEST_RE 同源）：`sha256:<64hex 小写>` 形合规即 Ok。
///
/// # Errors
///
/// 缺前缀、段长错或大写。
pub fn validate_digest(s: &str) -> Result<(), String> {
    let ok = s.strip_prefix("sha256:").is_some_and(|h| {
        h.len() == 64
            && h.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    });
    if ok {
        Ok(())
    } else {
        Err(format!("digest 须 sha256:<64hex 小写>，得 {s}"))
    }
}

/// issue 开单的 dry-run（#57 G6 评审强制项，账本面保留）：本地同规校验加
/// 将发送 body 与签名基形预览（ts/nonce/idem 占位），零网络零签名——账本
/// 只增不可撤且 per-key 日配额，误发测试单不可回收，契约实弹先走这里。
///
/// # Errors
///
/// 本地校验失败（title 空或超 200、kind 出集）；不含任何网络类错误。
pub fn issue_open_dry_run(
    title: &str,
    kind: &str,
    acceptance: &str,
    note: Option<&str>,
) -> Result<Value, String> {
    let title = title.trim();
    if title.is_empty() || title.chars().count() > 200 {
        return Err(format!(
            "title 必填且至多 200 字符（trim 后），得 {}",
            title.chars().count()
        ));
    }
    if !ISSUE_KINDS.contains(&kind) {
        return Err(format!("kind 仅 bug|improvement，得 {kind}"));
    }
    let mut body = json!({ "title": title, "kind": kind, "acceptance": acceptance });
    if let Some(n) = note {
        body["body"] = json!(n);
    }
    let path = format!("/repos/{REPO_ID}/issues");
    let body_text = serde_json::to_string(&body).unwrap_or_default();
    let base = signature_base("POST", &path, "<ts>", "<nonce>", "<idem>", &body_text);
    Ok(json!({
        "dryRun": true,
        "method": "POST",
        "url": format!("{}{path}", ledger_base()),
        "body": body,
        "signatureBase": base,
        "note": "零网络零签名：占位头不入账，确认后去 --dry-run 实发"
    }))
}

/// issue 开单（POST /repos/`<id>`/issues）：title trim 后 1 至 200，kind 缺
/// 省 bug，acceptance 验收条件（关单 result 引 digest 即完成判据）。
///
/// # Errors
///
/// 失败返回 `String` 错误（本地校验、网络与解析类、服务端 error 透传）。
pub fn issue_open(
    title: &str,
    kind: &str,
    acceptance: &str,
    note: Option<&str>,
) -> Result<Value, String> {
    let title = title.trim();
    if title.is_empty() || title.chars().count() > 200 {
        return Err(format!(
            "title 必填且至多 200 字符（trim 后），得 {}",
            title.chars().count()
        ));
    }
    if !ISSUE_KINDS.contains(&kind) {
        return Err(format!("kind 仅 bug|improvement，得 {kind}"));
    }
    let mut body = json!({ "title": title, "kind": kind, "acceptance": acceptance });
    if let Some(n) = note {
        body["body"] = json!(n);
    }
    post_signed(&format!("/repos/{REPO_ID}/issues"), &body)
}

/// issue 事件（POST /repos/`<id>`/issues/`<n>`/events）：type 见
/// [`ISSUE_EVENT_TYPES`]；payload 对象随 type（status 带 to；result 带
/// digest）。status=done 由服务端校验须先有 result。
///
/// # Errors
///
/// 失败返回 `String` 错误（本地校验、网络与解析类、服务端 error 透传）。
pub fn issue_event(
    n: u64,
    ev_type: &str,
    payload: Value,
    note: Option<&str>,
) -> Result<Value, String> {
    if !ISSUE_EVENT_TYPES.contains(&ev_type) {
        return Err(format!(
            "type 仅 {}，得 {ev_type}",
            ISSUE_EVENT_TYPES.join("|")
        ));
    }
    let mut body = json!({ "type": ev_type, "payload": payload });
    if let Some(b) = note {
        body["body"] = json!(b);
    }
    post_signed(&format!("/repos/{REPO_ID}/issues/{n}/events"), &body)
}

/// [`issue_event`] 的确定性幂等键形（评审 G1）：键 = sha256hex(流加类型
/// 加内容锚)，重试被服务端幂等去重不重复追加。
fn issue_event_with_idem(
    n: u64,
    ev_type: &str,
    payload: Value,
    note: Option<&str>,
    anchor: &str,
) -> Result<Value, String> {
    if !ISSUE_EVENT_TYPES.contains(&ev_type) {
        return Err(format!(
            "type 仅 {}，得 {ev_type}",
            ISSUE_EVENT_TYPES.join("|")
        ));
    }
    let mut body = json!({ "type": ev_type, "payload": payload });
    if let Some(b) = note {
        body["body"] = json!(b);
    }
    let idem = sha256_hex(format!("browse-issue-{n}-{ev_type}-{anchor}").as_bytes());
    post_signed_with_idem(
        &format!("/repos/{REPO_ID}/issues/{n}/events"),
        &body,
        Some(&idem),
    )
}

/// issue 关单链（result 引 digest 先行，status to=done 收尾；REQ-063 关
/// 单完成判据）。返回两事件回执数组。
/// # Errors
///
/// 失败返回 `String` 错误（两段任一失败即止，已写段不回滚——账本只增）。
pub fn issue_close(n: u64, digest: &str, note: Option<&str>) -> Result<Vec<Value>, String> {
    validate_digest(digest)?;
    let mut payload = json!({ "digest": digest });
    if let Some(b) = note {
        payload["note"] = json!(b);
    }
    // 确定性幂等键（评审 G1）：首段成次段败后重跑，result 与 done 都按
    // 键幂等命中返 replay，不重复追加（账本只增语义保持）。
    let result = issue_event_with_idem(n, "result", payload, note, digest)?;
    let status = issue_event_with_idem(n, "status", json!({ "to": "done" }), None, digest)?;
    Ok(vec![result, status])
}

/// issue 列表（GET，家族形）：limit 缺省 100（钳制 1 至 100），before
/// keyset 游标，`more=1` 使 has_more 恒在（与 before 无关，服务端契约）。
///
/// # Errors
///
/// 失败返回 `String` 错误（网络与解析类、服务端 error 透传）。
pub fn issues_list(limit: u32, before: Option<&str>) -> Result<Value, String> {
    let limit = clamp_issue_limit(limit);
    let mut path = format!("/repos/{REPO_ID}/issues?limit={limit}&more=1");
    if let Some(b) = before
        && !b.trim().is_empty()
    {
        if !b.chars().all(|c| c.is_ascii_digit()) {
            return Err(format!("before 须数字 issue 号，得 {b}"));
        }
        path.push_str(&format!("&before={b}"));
    }
    get_json(&path)
}

/// issue 详情（GET /repos/`<id>`/issues/`<n>`）：projection 加 timeline。
///
/// # Errors
///
/// 失败返回 `String` 错误（网络与解析类、服务端 error 透传）。
pub fn issue_show(n: u64) -> Result<Value, String> {
    get_json(&format!("/repos/{REPO_ID}/issues/{n}"))
}

/// artifact 发布（POST /repos/`<id>`/artifacts）：digest = 正文或记录哈希
/// （库不收二进制实体）；deps[] 记依赖出处（回溯链即证据链）。
///
/// # Errors
///
/// 失败返回 `String` 错误（本地校验、网络与解析类、服务端 error 透传）。
#[allow(clippy::too_many_arguments)]
pub fn artifact_publish(
    name: &str,
    kind: &str,
    digest: &str,
    version: Option<&str>,
    git_range: Option<&str>,
    deps: &[String],
    summary: Option<&str>,
    outcome: Option<&str>,
    git_sha: Option<&str>,
) -> Result<Value, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 200 {
        return Err(format!(
            "name 必填且至多 200 字符（trim 后），得 {}",
            name.chars().count()
        ));
    }
    if !ARTIFACT_KINDS.contains(&kind) {
        return Err(format!("kind 仅 {}，得 {kind}", ARTIFACT_KINDS.join("|")));
    }
    validate_digest(digest)?;
    let mut payload = json!({ "name": name, "kind": kind, "digest": digest, "deps": deps });
    if let Some(v) = version {
        payload["version"] = json!(v);
    }
    if let Some(g) = git_range {
        payload["git_range"] = json!(g);
    }
    if let Some(s) = summary {
        payload["summary"] = json!(s);
    }
    if let Some(o) = outcome {
        payload["outcome"] = json!(o);
    }
    if let Some(g) = git_sha {
        payload["git_sha"] = json!(g);
    }
    post_signed(&format!("/repos/{REPO_ID}/artifacts"), &payload)
}

/// artifact 事件（POST /repos/`<id>`/artifacts/`<id>`/attestations）：type 见
/// [`ATTEST_TYPES`]；payload 对象（attest_prod 强制 env=prod、promote 缺
/// name 时服务端补 artifact 名）。
///
/// # Errors
///
/// 失败返回 `String` 错误（本地校验、网络与解析类、服务端 error 透传）。
pub fn artifact_attest(
    artifact_id: &str,
    ev_type: &str,
    payload: Value,
    note: Option<&str>,
) -> Result<Value, String> {
    validate_artifact_id(artifact_id)?;
    if !ATTEST_TYPES.contains(&ev_type) {
        return Err(format!("type 仅 {}，得 {ev_type}", ATTEST_TYPES.join("|")));
    }
    let mut body = json!({ "type": ev_type, "payload": payload });
    if let Some(b) = note {
        body["body"] = json!(b);
    }
    post_signed(
        &format!("/repos/{REPO_ID}/artifacts/{artifact_id}/attestations"),
        &body,
    )
}

/// artifact 列表（GET）：current=1 投影当前有效集，env 过滤 dev|prod。
///
/// # Errors
///
/// 失败返回 `String` 错误（网络与解析类、服务端 error 透传）。
pub fn artifacts_list(current: bool, env: Option<&str>) -> Result<Value, String> {
    let mut path = format!("/repos/{REPO_ID}/artifacts?");
    if current {
        path.push_str("current=1&");
    }
    if let Some(e) = env.filter(|e| !e.is_empty()) {
        path.push_str(&format!("env={e}&"));
    }
    get_json(path.trim_end_matches('&'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_base_is_seven_newline_joined_rows() {
        // 服务端 signatureBase 同源向量：v1 加方法加路径加 ts 加 nonce
        // 加 idem 加 body sha256hex，各行换行连。
        let body = r#"{"title":"t"}"#;
        let expect_body_sha = {
            let mut h = Sha256::new();
            h.update(body.as_bytes());
            format!("{:x}", h.finalize())
        };
        assert_eq!(
            signature_base(
                "POST",
                "/repos/x/issues",
                "1700000000",
                "nonce1",
                "idem1",
                body
            ),
            format!("v1\nPOST\n/repos/x/issues\n1700000000\nnonce1\nidem1\n{expect_body_sha}")
        );
        assert_eq!(
            signature_base("POST", "/p", "1", "n", "i", "")
                .matches('\n')
                .count(),
            6
        );
    }

    #[test]
    fn key_id_is_sha256_of_canonical_jwk_const() {
        let _g = ();
        // kid = sha256hex(JWK 常量)；常量即字母键序紧凑形（单一真相）。
        let mut h = Sha256::new();
        h.update(PUBKEY_JWK.as_bytes());
        let expect = format!("{:x}", h.finalize());
        assert_eq!(key_id(), expect);
        assert_eq!(key_id().len(), 64);
        // JWK 常量形自检：字母键序加紧凑（无空白）。
        assert!(PUBKEY_JWK.starts_with("{\"crv\":\"Ed25519\",\"kty\":\"OKP\",\"x\":\""));
        assert!(!PUBKEY_JWK.contains(' '));
    }

    #[test]
    fn artifact_id_validation_rejects_truncated_form() {
        // 实弹翻车向量（2026-09-20）：真值首段双 b 八位 66857bb2，截断值丢
        // 一字符成 66857b2；形校验本地拦，免上游 404 往返。
        assert!(validate_artifact_id("66857bb2-d1ae-4977-97df-8ab41cc37399").is_ok());
        assert!(validate_artifact_id("66857b2-d1ae-4977-97df-8ab41cc37399").is_err());
        assert!(validate_artifact_id("66857bb2-dae-4977-97df-8ab41cc37399").is_err());
        assert!(validate_artifact_id("").is_err());
        assert!(
            validate_artifact_id("66857BB2-D1AE-4977-97DF-8AB41CC37399").is_err(),
            "大写拒（服务端小写形）"
        );
    }

    #[test]
    fn digest_validation_accepts_lowercase_hex_only() {
        assert!(validate_digest(&format!("sha256:{}", "a".repeat(64))).is_ok());
        assert!(validate_digest(&format!("sha256:{}", "0".repeat(64))).is_ok());
        assert!(validate_digest("sha256:SHORT").is_err());
        assert!(
            validate_digest(&format!("sha256:{}", "A".repeat(64))).is_err(),
            "大写拒"
        );
        assert!(
            validate_digest(&"a".repeat(64).to_string()).is_err(),
            "缺前缀拒"
        );
        assert!(validate_digest(&format!("sha512:{}", "a".repeat(64))).is_err());
    }

    #[test]
    fn kind_sets_match_server_contract() {
        assert!(ISSUE_KINDS.contains(&"bug") && ISSUE_KINDS.contains(&"improvement"));
        assert!(ISSUE_EVENT_TYPES.contains(&"result") && ISSUE_EVENT_TYPES.contains(&"status"));
        assert!(ISSUE_STATUSES.contains(&"done"));
        assert_eq!(ARTIFACT_KINDS.len(), 15);
        assert!(ARTIFACT_KINDS.contains(&"experience") && ARTIFACT_KINDS.contains(&"lesson"));
        assert!(ATTEST_TYPES.contains(&"attest_dev") && ATTEST_TYPES.contains(&"promote"));
    }

    #[test]
    fn keygen_write_and_load_roundtrip_isolated() {
        unsafe {
            let _env_guard = crate::TEST_ENV_LOCK.lock().unwrap();
            // 自封闭（评审 F1 补）：先清外部注入的 BROWSE_LEDGER_PRIVATE_KEY
            // ——load 的 env 通道优先于密档，外部 env 会让 via_file 实走 env、
            // force 断言的旧 kid 也算错，单靠锁防不住前置注入。
            std::env::remove_var("BROWSE_LEDGER_PRIVATE_KEY");
            // keygen 落密档（0600）加 load 双通道（env 与文件）往返同钥；隔
            // 离机制 = BROWSE_NAME 传绝对路径（state_dir_for 对绝对路径直接
            // 取给定值，前缀 ~/.browse-rs 被丢弃，评审 G7 口径）。
            let root = std::env::temp_dir().join(format!(
                "browse-ledger-key-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis()
            ));
            std::env::set_var("BROWSE_NAME", &root);
            let (kid, jwk, old) = keygen_write(false).unwrap();
            assert_eq!(kid.len(), 64);
            assert!(old.is_none(), "首代无旧 kid");
            assert!(jwk.contains("Ed25519"));
            let via_file = load_signing_key().unwrap();
            // env 通道同钥。
            let seed_b64 = {
                use base64::Engine;
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(via_file.to_bytes())
            };
            std::env::set_var("BROWSE_LEDGER_PRIVATE_KEY", &seed_b64);
            let via_env = load_signing_key().unwrap();
            assert_eq!(via_file.to_bytes(), via_env.to_bytes());
            // 坏 seed 拒。
            std::env::set_var("BROWSE_LEDGER_PRIVATE_KEY", "tooshort");
            assert!(load_signing_key().is_err());
            // 覆盖守卫：在位密档非 force 拒、force 过（评审 G2b）。
            std::env::remove_var("BROWSE_LEDGER_PRIVATE_KEY");
            assert!(keygen_write(false).is_err());
            let (_, _, old2) = keygen_write(true).unwrap();
            assert_eq!(old2, Some(kid.clone()), "force 带出旧 kid");
            std::env::remove_var("BROWSE_LEDGER_PRIVATE_KEY");
            std::env::remove_var("BROWSE_NAME");
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    #[test]
    fn baked_jwk_pairs_with_local_keyfile() {
        let _env_guard = crate::TEST_ENV_LOCK.lock().unwrap();
        // REQ-063 裁 2：公钥常量须与本地密档配对，否则写入全体 401；自封
        // 闭（评审 F1 补）：先清外部注入的 env——本测试对象是密档面与内置
        // JWK 的配对，env 通道的运行时行为不是断言对象，外部 env 只会造成
        // 「不配对」假故障。
        unsafe { std::env::remove_var("BROWSE_LEDGER_PRIVATE_KEY") };
        match pairing_ok() {
            Ok(Some(true)) => {}
            Ok(Some(false)) => panic!(
                "内置公钥 JWK 与本地私钥不配对（重跑 ledger keygen --force 并烧新 JWK 进常量）"
            ),
            Ok(None) => {} // 密档缺位（CI 态合法）
            Err(e) => panic!("{e}"),
        }
    }

    #[test]
    fn truncation_hint_points_to_ledger_web_face() {
        let h = issue_list_truncation_hint(100);
        assert!(
            h.starts_with("issue.list.truncated=limit-reached limit=100"),
            "{h}"
        );
        assert!(h.contains("--before"), "{h}");
        assert!(h.contains("ledger"), "{h}");
        assert!(h.contains(REPO_ID), "{h}");
    }
}
