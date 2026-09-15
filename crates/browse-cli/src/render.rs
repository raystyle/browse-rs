//! 求值结果的打印面：大值自动落盘（artifact/checkpoint 的降级形态，
//! 用户裁定）。daemon 与 vars 表不动，只保护 agent 上下文不被 10MB JSON 淹没。

use serde_json::Value;
use std::path::PathBuf;

/// 触发落盘的阈值（字节）。序列化后的打印串超过即落盘。
pub const DROP_THRESHOLD: usize = 32 * 1024;

/// 预览长度（字符）。
const PREVIEW_CHARS: usize = 160;

/// 落盘目录：`%USERPROFILE%\.browse-rs\drops`。
fn drops_dir() -> PathBuf {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".browse-rs").join("drops")
}

/// 渲染求值结果；超阈值时写文件并返回提示行（含路径与预览），
/// 未超阈值返回正常渲染。
///
/// # Errors
///
/// 落盘失败（目录不可写）。
///
/// # Examples
///
/// ```
/// use serde_json::json;
/// // 小值正常渲染
/// assert_eq!(browse_cli::render::render_or_drop_sync(&json!("hi")).unwrap(), "hi");
/// // 大值落盘（同步测试路径）
/// let big = json!("x".repeat(browse_cli::render::DROP_THRESHOLD + 1));
/// let line = browse_cli::render::render_or_drop_sync(&big).unwrap();
/// assert!(line.contains("__dropped"), "{line}");
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
    std::fs::write(&path, &rendered)?;
    let preview: String = rendered.chars().take(PREVIEW_CHARS).collect();
    let suffix = if rendered.chars().count() > PREVIEW_CHARS {
        "…"
    } else {
        ""
    };
    Ok(format!(
        "{{\"__dropped\": true, \"bytes\": {}, \"path\": {}, \"preview\": {}}}",
        rendered.len(),
        Value::String(path.display().to_string()),
        Value::String(format!("{preview}{suffix}"))
    ))
}
