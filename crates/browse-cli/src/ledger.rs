//! 账本薄适配层（REQ-063；总台修正令 2026-09-20 收口）：签名道与只增面
//! 全在 ledger-client crate（github.com/raystyle/ledger-rs v0.1.1，全舰队
//! 唯一实现；v0.1.0 有 URL 拼接舰队级缺陷已避），本层只留本仓身份面
//! （公钥 JWK 常量与 kid 派生）、密档管理
//! （base64url seed，env `BROWSE_LEDGER_PRIVATE_KEY` 或本地密档双通道）、
//! 命令面本地校验、`--dry-run` 载荷预览与 #52 家族截断提示。CLI 只增不关
//! 不删：issue close 与 artifact promote/demote/supersede 面已移除，关闭
//! 与删除唯一道 = 开发工作台经 herdr 委托 omc 工位执行（`omc ledger issue
//! status <repo> <n> <to>` 与 `omc ledger issue delete`）。真源 =
//! ledger.ohmygh.com（替代 issues.ohmygh.com 客户端面；旧服务只读保役）。

use std::path::PathBuf;

use base64::Engine;
use ed25519_dalek::SigningKey;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// 本仓账本身份（REQ-063：repo_id = 规范化 remote）。
pub const REPO_ID: &str = "github.com/raystyle/browse-rs";

/// 本仓公钥 JWK（REQ-063 裁 2：常量集成进 CLI；字母键序紧凑形，kid 即
/// 其 sha256hex，与舰队派生约定一致）。私钥恒在提交侧（env 或本地密档），
/// 不进仓。
pub const PUBKEY_JWK: &str =
    "{\"crv\":\"Ed25519\",\"kty\":\"OKP\",\"x\":\"vGpYNa-iVpc3k00mG3aPZbk94NlHeWEABOqqal65Pag\"}";

/// issue kind 集（服务端 ISSUE_KINDS 同源，本地预检省一次网络往返）：bug
/// （BUG 错误任务）与 improvement（改进优化任务）。
pub const ISSUE_KINDS: &[&str] = &["bug", "improvement"];

/// ledger 密档根（`~/.browse-rs/ledger/`，BROWSE_NAME 命名实例各一份）。
fn ledger_home() -> Result<std::path::PathBuf, String> {
    Ok(browse_core::paths::state_dir().join("ledger"))
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

/// 私钥密档路径（`~/.browse-rs/ledger/ed25519.key`，内容 = base64url 32 字节
/// seed；0600 由 keygen 落）。
///
/// # Errors
///
/// 密档根解析失败。
pub fn private_key_path() -> Result<PathBuf, String> {
    Ok(crate::ledger::ledger_home()?.join("ed25519.key"))
}

fn b64url(seed: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(seed)
}

/// 密钥对生成（一次性或轮换）：写私钥密档（0600）并返回 (kid, JWK, 旧
/// kid)；私钥不打印不进 argv，公钥 JWK 供总台在册（在册后方可写入）。
/// 在位密档非 `--force` 即拒（防静默销毁）。
///
/// # Errors
///
/// 失败返回 `String` 错误（路径与写入类）。
pub fn keygen_write(force: bool) -> Result<(String, String, Option<String>), String> {
    let path = private_key_path()?;
    // 覆盖守卫（评审 G2b）：在册私钥静默销毁 = 后续写入全 401 且旧钥不可
    // 恢复；非 force 即拒，force 时先带出旧 kid 供对账。
    let old_kid = if path.exists() {
        let old = load_keypair().ok().map(|k| k.key_id.clone());
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
    // 本地生成 SigningKey（seed 须落盘，KeyPair 不暴露私钥），身份面经标准
    // crate 构造（kid 派生约定同源）；密档保持本仓 base64url seed 形，在册
    // 密钥不受收口影响。
    let signing = SigningKey::generate(&mut rand::rngs::OsRng);
    let seed_out = b64url(&signing.to_bytes());
    let kp = ledger_client::KeyPair::from_signing(signing);
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    // 原子 0600（评审 G2a）：OpenOptions 带模式一次落，消两步间的 0644 窗。
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true).mode(0o600);
        let mut f = opts
            .open(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        f.write_all(seed_out.as_bytes())
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    #[cfg(not(unix))]
    std::fs::write(&path, seed_out.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((kp.key_id.clone(), kp.public_jwk.clone(), old_kid))
}

/// 载入账本身份：env `BROWSE_LEDGER_PRIVATE_KEY`（base64url seed）优先，
/// 次本地密档；构造标准 crate 的 [`ledger_client::KeyPair`]（kid 与 JWK 由
/// 其按舰队约定派生）。不进 argv 不进仓。
///
/// # Errors
///
/// env 值或密档内容非 base64url 32 字节 seed，或密档不可读。
pub fn load_keypair() -> Result<ledger_client::KeyPair, String> {
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
    Ok(ledger_client::KeyPair::from_signing(
        SigningKey::from_bytes(&arr),
    ))
}

/// 组装标准客户端（签名道与只增面在 ledger-client；本仓身份面注入）。
///
/// # Errors
///
/// 私钥载入失败（见 [`load_keypair`]）。
pub fn client() -> Result<ledger_client::Ledger, String> {
    Ok(ledger_client::Ledger::new(REPO_ID, load_keypair()?))
}

/// 本地私钥与内置公钥 JWK 的配对自检（评审 G3）：密档/env 缺位回 None
/// （无法判），在位回配对与否——不配对则写入会全体 401 且难归因。
///
/// # Errors
///
/// 内置 JWK 常量解析失败（形变即测试红，运行时不可达）。
pub fn pairing_ok() -> Result<Option<bool>, String> {
    let kp = match load_keypair() {
        Ok(k) => k,
        Err(_) => return Ok(None),
    };
    Ok(Some(kp.public_jwk == PUBKEY_JWK))
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
        ledger_client::BASE_URL
    )
}

/// issue 开单入参本地预检（实发腿与 dry-run 同规，收口批评审 F1）：坏
/// 入参本地拦（exit 2 语义），免打到服务端吃 400 且损耗 per-key 日配额。
///
/// # Errors
///
/// title trim 后空或超 200 字符；kind 出 [`ISSUE_KINDS`] 集。
pub fn validate_issue_open(title: &str, kind: &str) -> Result<(), String> {
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
    Ok(())
}

/// artifact 发布入参本地预检（收口批评审 F1）：name trim 后 1 至 200，
/// 服务端同规；坏入参本地拦免配额损耗。
///
/// # Errors
///
/// name trim 后空或超 200 字符。
pub fn validate_artifact_publish(name: &str) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 200 {
        return Err(format!(
            "name 必填且至多 200 字符（trim 后），得 {}",
            name.chars().count()
        ));
    }
    Ok(())
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
    validate_issue_open(title, kind)?;
    let title = title.trim();
    let mut body = json!({ "title": title, "kind": kind, "acceptance": acceptance });
    if let Some(n) = note {
        body["body"] = json!(n);
    }
    let path = format!("/repos/{REPO_ID}/issues");
    let body_text = serde_json::to_string(&body).unwrap_or_default();
    let base = [
        "v1",
        "POST",
        &path,
        "<ts>",
        "<nonce>",
        "<idem>",
        &sha256_hex(body_text.as_bytes()),
    ]
    .join("\n");
    Ok(json!({
        "dryRun": true,
        "method": "POST",
        "url": format!("{}{path}", ledger_client::BASE_URL),
        "body": body,
        "signatureBase": base,
        "note": "零网络零签名：占位头不入账，确认后去 --dry-run 实发"
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kid_is_sha256_of_canonical_jwk_const() {
        let _env_guard = crate::TEST_ENV_LOCK.lock().unwrap();
        // kid = sha256hex(JWK 常量)；常量即字母键序紧凑形（单一真相），与
        // ledger-client 的舰队派生约定同源。
        let mut h = Sha256::new();
        h.update(PUBKEY_JWK.as_bytes());
        let expect = format!("{:x}", h.finalize());
        assert_eq!(key_id(), expect);
        assert_eq!(key_id().len(), 64);
        assert!(PUBKEY_JWK.starts_with("{\"crv\":\"Ed25519\",\"kty\":\"OKP\",\"x\":\""));
        assert!(!PUBKEY_JWK.contains(' '));
    }

    #[test]
    fn kind_sets_match_standard_crate() {
        // 本地 kind 预检集与标准 crate 同源（服务端 ISSUE_KINDS）。
        assert!(ISSUE_KINDS.contains(&"bug") && ISSUE_KINDS.contains(&"improvement"));
        // 只增面：attest 三型，无 promote/demote/supersede（修正令收口）。
        assert_eq!(
            ledger_client::ATTEST_TYPES,
            &["attest_dev", "attest_prod", "verification_failed"]
        );
        assert_eq!(ledger_client::ARTIFACT_KINDS.len(), 15);
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
    fn dry_run_is_local_only_with_signature_base_preview() {
        let v = issue_open_dry_run("探针", "bug", "零入账", Some("b")).unwrap();
        assert_eq!(v["dryRun"], json!(true));
        assert!(v["url"].as_str().unwrap().contains("ledger.ohmygh.com"));
        assert!(
            v["signatureBase"]
                .as_str()
                .unwrap()
                .starts_with("v1\nPOST\n/repos/")
        );
        // 签名基七行对账（评审 G8）：crate 未导出构造，本层内联形以行数锁
        // 漂移（v1/POST/路径/ts/nonce/idem/body-hash 各占一行，六换行）
        assert_eq!(
            v["signatureBase"].as_str().unwrap().matches('\n').count(),
            6
        );
        assert!(issue_open_dry_run("", "bug", "a", None).is_err());
        assert!(issue_open_dry_run("t", "chore", "a", None).is_err());
    }

    #[test]
    fn keygen_write_and_load_roundtrip_isolated() {
        unsafe {
            let _env_guard = crate::TEST_ENV_LOCK.lock().unwrap();
            // 自封闭（评审 F1 补）：先清外部注入的 BROWSE_LEDGER_PRIVATE_KEY
            // ——load 的 env 通道优先于密档，外部 env 会让断言算错，单靠锁
            // 防不住前置注入。
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
            let via_file = load_keypair().unwrap();
            assert_eq!(via_file.key_id, kid, "密档往返同 kid");
            // env 通道同钥。
            let seed_b64 =
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(seed_from_keyfile());
            std::env::set_var("BROWSE_LEDGER_PRIVATE_KEY", &seed_b64);
            let via_env = load_keypair().unwrap();
            assert_eq!(via_env.key_id, via_file.key_id);
            // 坏 seed 拒。
            std::env::set_var("BROWSE_LEDGER_PRIVATE_KEY", "tooshort");
            assert!(load_keypair().is_err());
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

    /// 测试侧取 seed：KeyPair 不暴露私钥，经密档文件读（roundtrip 内部用）。
    fn seed_from_keyfile() -> [u8; 32] {
        let p = private_key_path().unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        let seed = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(text.trim().as_bytes())
            .unwrap();
        seed.try_into().unwrap()
    }

    #[test]
    fn baked_jwk_pairs_with_local_keyfile() {
        let _env_guard = crate::TEST_ENV_LOCK.lock().unwrap();
        // REQ-063 裁 2：公钥常量须与本地密档配对，否则写入全体 401；自封
        // 闭（评审 F1 补）：先清外部注入的 env——本测试对象是密档面与内置
        // JWK 的配对，env 通道的运行时行为不是断言对象。
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
