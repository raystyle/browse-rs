//! 技能触发层（#50/#51）：goto 导航回执的条件附加面。知识全文存
//! workspace 单仓（[`crate::workspace`]，github.com/raystyle/browse_workspace），
//! 本模块只做「点名」：命中附加清单与 hint（读全文命令），未命中或
//! 关闭时一键不加，回执与现状逐字节一致。

use serde_json::{Value, json};

/// 触发开关的关值判式（纯函数）：值为 `0` 或 `false` 才关（opt-out，
/// 未设或任何其他值都开）。与 `BROWSE_NO_ATTACH` 的 opt-in
/// （`1`/`true` 才生效）方向相反，镜像前代 bh 的 `BH_DOMAIN_SKILLS=0`。
fn is_off(v: Option<std::ffi::OsString>) -> bool {
    v.map(|s| s.to_string_lossy().trim().to_ascii_lowercase())
        .is_some_and(|s| s == "0" || s == "false")
}

/// #50 域名层是否开启：`BROWSE_DOMAIN_SKILLS` 未设或非 0/false 即开。
/// 逐调用读 env 不缓存（paths.rs 先例；也保 e2e 可注入临时仓）。
pub fn domain_skills_enabled() -> bool {
    !is_off(std::env::var_os("BROWSE_DOMAIN_SKILLS"))
}

/// URL 到域名段（#50，纯函数）：先做 http(s) 门禁（[`cdp::session::url_host`]
/// 对 data:/about: 这类形返回的是 scheme 段，不能当 host 用），再取 host、
/// 剥 `www.` 前缀、取首个 `.` 前段。非 http(s) 或空 host 返回 `None`。
/// 多词域名不做特判（bbc.co.uk 的段是 bbc，够用口径）。
///
/// # Examples
///
/// ```
/// assert_eq!(browse_core::skills::domain_segment("https://x.com/a"), Some("x".into()));
/// assert_eq!(
///     browse_core::skills::domain_segment("http://www.github.com/"),
///     Some("github".into())
/// );
/// assert_eq!(browse_core::skills::domain_segment("data:text/html,x"), None);
/// ```
pub fn domain_segment(url: &str) -> Option<String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return None;
    }
    let host = cdp::session::url_host(url);
    let host = host.strip_prefix("www.").unwrap_or(&host);
    let seg = host.split('.').next().unwrap_or("");
    if seg.is_empty() || !seg.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        None
    } else {
        Some(seg.to_string())
    }
}

/// 列 `<ws>/domain-skills/<段>/` 的技能文件名（#50，纯函数）：只收
/// .md/.txt，排序保回执确定（read_dir 顺序不保证），封顶
/// [`crate::workspace::DOMAIN_FILES_CAP`]；目录缺失返回空 vec。
///
/// # Examples
///
/// ```
/// // 缺目录返回空（不报错：未装仓是常态）
/// let v = browse_core::skills::list_domain_skills(
///     std::path::Path::new("/nonexistent-browse-ws"), "x");
/// assert!(v.is_empty());
/// ```
pub fn list_domain_skills(ws: &std::path::Path, segment: &str) -> Vec<String> {
    crate::workspace::domain_segment_files(ws, segment)
}

/// 域名层 hint 字段值（#50）：读全文命令字串。
pub fn domain_hint(segment: &str) -> String {
    format!("browse workspace site {segment}")
}

/// goto 回执的技能附加入口（crate 内缝，#50/#51）：按开关分流各层；
/// 任何失败（目录列举失败、后续页面探测失败）静默返回，绝不让 goto
/// 失败；未命中或关闭时一键不加（回执与现状逐字节一致）。调用点在
/// [`crate::semantic::goto`] 的回执构建处，elapsedMs 在此之前已固化。
pub(crate) async fn augment_goto(_s: &cdp::Session, receipt: &mut Value) {
    if !domain_skills_enabled() {
        return;
    }
    let url = receipt.get("url").and_then(Value::as_str).unwrap_or("");
    let Some(seg) = domain_segment(url) else {
        return;
    };
    let files = list_domain_skills(&crate::paths::workspace_dir(), &seg);
    if files.is_empty() {
        return;
    }
    if let Some(obj) = receipt.as_object_mut() {
        obj.insert("domain_skills".into(), json!(files));
        obj.insert("domain_skills_hint".into(), json!(domain_hint(&seg)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// is_off：0/false 关，未设、1、任意串都开。
    #[test]
    fn switch_off_values() {
        assert!(is_off(Some("0".into())));
        assert!(is_off(Some("false".into())));
        assert!(is_off(Some("FALSE".into())));
        assert!(!is_off(None));
        assert!(!is_off(Some("1".into())));
        assert!(!is_off(Some("off".into())));
    }

    /// domain_segment：命中、去 www.、首段、大小写、非 http(s)。
    #[test]
    fn segment_extraction() {
        assert_eq!(domain_segment("https://x.com/a"), Some("x".into()));
        assert_eq!(
            domain_segment("http://www.github.com/"),
            Some("github".into())
        );
        assert_eq!(
            domain_segment("https://news.ycombinator.com/item?id=1"),
            Some("news".into())
        );
        assert_eq!(domain_segment("https://X.COM"), Some("x".into()));
        assert_eq!(domain_segment("data:text/html,<p>x</p>"), None);
        assert_eq!(domain_segment("about:blank"), None);
        assert_eq!(domain_segment("https://"), None);
    }

    /// list_domain_skills：排序、扩展过滤、封顶 10。
    #[test]
    fn listing_sorted_filtered_capped() {
        let dir = std::env::temp_dir().join(format!("browse-sk-{}", std::process::id()));
        let seg = dir.join("domain-skills").join("x");
        std::fs::create_dir_all(&seg).unwrap();
        for i in 0..11 {
            std::fs::write(seg.join(format!("f{i:02}.md")), "# f").unwrap();
        }
        std::fs::write(seg.join("skip.js"), "// x").unwrap();
        let files = list_domain_skills(&dir, "x");
        assert_eq!(files.len(), 10, "封顶 10：{files:?}");
        assert_eq!(files[0], "f00.md");
        assert!(!files.contains(&"skip.js".to_string()), "非 md/txt 滤除");
        assert!(list_domain_skills(&dir, "nope").is_empty(), "缺段空表");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
