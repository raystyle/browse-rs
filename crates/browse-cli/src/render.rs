//! 求值结果的打印面：大值自动落盘（artifact/checkpoint 的降级形态，
//! 用户裁定）。daemon 与 vars 表不动，只保护 agent 上下文不被 10MB JSON 淹没。

use serde_json::Value;
use std::path::PathBuf;

/// 序列化后的打印串超过本阈值（字节）即落盘，保护 agent 上下文。
pub const DROP_THRESHOLD: usize = 32 * 1024;

/// 落盘提示行里保留的预览长度（字符数）。
const PREVIEW_CHARS: usize = 160;

/// 返回落盘目录 `<state>/drops`（命名实例见 [`browse_core::paths`]）。
fn drops_dir() -> PathBuf {
    browse_core::paths::state_dir().join("drops")
}

/// 渲染求值结果；超阈值时写文件并返回提示行（含路径与预览），
/// 未超阈值返回正常渲染。落盘文件写原文（字符串不带展示引号），stdout
/// 展示面才带类型区分（#21）；回执 `bytes` 是工件字节（非展示形态长度）。
///
/// # Errors
///
/// 落盘失败（目录不可写）。
///
/// # Examples
///
/// ```
/// use serde_json::json;
/// // 小值正常渲染（字符串带引号，与对象输出可区分）
/// assert_eq!(
///     browse_cli::render::render_or_drop_sync(&json!("hi")).unwrap(),
///     "\"hi\""
/// );
/// // 大值落盘（同步测试路径）；回执 bytes 是工件字节，不含展示引号
/// let big = json!("x".repeat(browse_cli::render::DROP_THRESHOLD + 1));
/// let line = browse_cli::render::render_or_drop_sync(&big).unwrap();
/// assert!(line.contains("__dropped"), "{line}");
/// let receipt: serde_json::Value = serde_json::from_str(&line).unwrap();
/// assert_eq!(
///     receipt["bytes"].as_u64(),
///     Some(browse_cli::render::DROP_THRESHOLD as u64 + 1)
/// );
/// ```
pub fn render_or_drop_sync(v: &Value) -> std::io::Result<String> {
    let rendered = browse_core::render_result(v);
    if rendered.len() <= DROP_THRESHOLD {
        return Ok(rendered);
    }
    let dir = drops_dir();
    std::fs::create_dir_all(&dir)?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let ext = match v {
        Value::Object(_) | Value::Array(_) => "json",
        _ => "txt",
    };
    let path = dir.join(format!("value-{ts}.{ext}"));
    // 文件是工件不是展示（#25.4 评审 F2）：两类都落原值——字符串落原文，
    // 容器落未脱敏的 JSON 序列化（rendered 已过面具，不能当工件）
    let raw = match v {
        Value::String(s) => s.clone(),
        other => serde_json::to_string(other).unwrap_or_else(|_| rendered.clone()),
    };
    std::fs::write(&path, &raw)?;
    let preview: String = rendered.chars().take(PREVIEW_CHARS).collect();
    let suffix = if rendered.chars().count() > PREVIEW_CHARS {
        "…"
    } else {
        ""
    };
    Ok(format!(
        "{{\"__dropped\": true, \"bytes\": {}, \"path\": {}, \"preview\": {}}}",
        raw.len(),
        Value::String(path.display().to_string()),
        Value::String(format!("{preview}{suffix}"))
    ))
}
