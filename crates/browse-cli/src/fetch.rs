//! 一次性只读抓取（#50）：HTTP 优先，三条件升级引擎，markdown 直出。
//!
//! - HTTP 腿：reqwest 直取（零浏览器成本），判升级三条件：正文空、命中
//!   墙词、正文少于 20 词。
//! - 引擎腿：经 daemon POST /eval 走 goto 加页内抽取（启发式标题加段落，
//!   非 Readability；v1 口径已在 surface 披露）。

use anyhow::{Result, anyhow, bail};
use serde_json::Value;
use serde_json::json;

/// 升级判定的三条件（#50）：正文空、墙词、少于 20 词。
/// 纯函数供单测锁。
pub fn needs_upgrade(body_text: &str) -> Option<&'static str> {
    let t = body_text.trim();
    if t.is_empty() {
        return Some("empty");
    }
    const WALLS: [&str; 8] = [
        "just a moment",
        "attention required",
        "checking your browser",
        "verify you are human",
        "enable javascript",
        "access denied",
        "unusual traffic",
        "captcha",
    ];
    let low = t.to_lowercase();
    if WALLS.iter().any(|w| low.contains(w)) {
        return Some("wall-word");
    }
    let words = low.split_whitespace().count();
    if words < 20 {
        return Some("thin-content");
    }
    None
}

/// HTML 粗抽正文文本（HTTP 腿的降级面）：剥 script/style 与标签，压空白。
fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut in_tag = false;
    let mut skip_until: Option<&str> = None;
    let lower = html.to_lowercase();
    let bytes = html.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let rest = &lower[i..];
        if let Some(end) = skip_until {
            if let Some(pos) = rest.find(end) {
                i += pos + end.len();
                skip_until = None;
                in_tag = false;
                continue;
            }
            break;
        }
        if !in_tag && lower[i..].starts_with("<script") {
            skip_until = Some("</script>");
            i += 7;
            continue;
        }
        if !in_tag && lower[i..].starts_with("<style") {
            skip_until = Some("</style>");
            i += 6;
            continue;
        }
        if !in_tag && lower[i..].starts_with("<title") {
            skip_until = Some("</title>");
            i += 6;
            continue;
        }
        let ch = html[i..].chars().next().unwrap_or(' ');
        if ch == '<' {
            in_tag = true;
        } else if ch == '>' {
            in_tag = false;
        } else if !in_tag {
            out.push(ch);
        }
        i += ch.len_utf8();
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 抓取入口：HTTP 优先，命中升级条件转引擎腿。
///
/// # Errors
///
/// 两腿全失败（HTTP 网络错且引擎腿也失败）；引擎腿的 goto/抽取错误原样
/// 上抛。
pub async fn fetch(url: &str, _markdown: bool, timeout_s: u64) -> Result<Value> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_s))
        .user_agent(concat!("browse/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let http = client.get(url).send().await;
    let (status, html) = match http {
        Ok(r) => {
            let s = r.status().as_u16();
            match r.text().await {
                Ok(t) => (s, t),
                Err(_) => (s, String::new()),
            }
        }
        Err(_) => (0, String::new()),
    };
    let title = html
        .to_lowercase()
        .find("<title")
        .and_then(|p| html[p..].find('>').map(|q| p + q + 1))
        .and_then(|start| html[start..].find("</title>").map(|e| (start, e)))
        .map(|(s, e)| html[s..s + e].trim().to_string())
        .unwrap_or_default();
    let text = strip_html(&html);
    let reason = if status == 0 {
        Some("http-error")
    } else {
        needs_upgrade(&text)
    };
    match reason {
        None => Ok(json!({
            "url": url, "via": "http", "status": status,
            "title": title,
            "text": text,
            "bytes": html.len(),
        })),
        Some(why) => {
            // 引擎腿：经 daemon 求值（懒拉起口径同 CLI 求值面）
            let extract = r#"JSON.stringify({title: document.title, text: (document.body ? document.body.innerText : '').trim()})"#;
            let code = format!(
                "await goto({url}, {{timeout: 20}}); return (await session.Runtime.evaluate({{expression: {expr}, returnByValue: true}})).result.value",
                url = serde_json::to_string(url)?,
                expr = serde_json::to_string(extract)?,
            );
            // G1：复用 CLI 求值面的 daemon 发现（含 BROWSE_NAME 派生端口），
            // 不只认 BROWSE_PORT 缺省；懒拉起同构（#49）：daemon 不在则
            // 自动拉起，冷启动 fetch 不再要求先手工 browse up
            crate::client::ensure_daemon_with_env(&crate::client::skills_passthrough_env()).await?;
            let bind = crate::client::daemon_bind();
            let resp = client
                .post(format!("http://{bind}/eval"))
                .json(&serde_json::json!({ "code": code }))
                .timeout(std::time::Duration::from_secs(timeout_s + 60))
                .send()
                .await
                .map_err(|e| {
                    anyhow!("引擎腿 daemon 不可达（{e}）；下一步：browse up 或修 BROWSE_PORT")
                })?;
            let v: Value = resp.json().await?;
            if v.get("ok") != Some(&json!(true)) {
                bail!(
                    "引擎腿失败：{}",
                    v.get("error").and_then(Value::as_str).unwrap_or("?")
                );
            }
            let inner = v
                .get("value")
                .and_then(Value::as_str)
                .and_then(|s| serde_json::from_str::<Value>(s).ok())
                .unwrap_or(json!({}));
            Ok(json!({
                "url": url, "via": "engine", "upgradedFrom": why, "httpStatus": status,
                "title": inner.get("title"),
                "text": inner.get("text"),
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #50 三条件分类器：空、墙词、薄内容各自命中；健康正文不升级。
    #[test]
    fn upgrade_conditions() {
        assert_eq!(needs_upgrade(""), Some("empty"));
        assert_eq!(needs_upgrade("   "), Some("empty"));
        assert_eq!(
            needs_upgrade("Please wait while we are checking your browser"),
            Some("wall-word")
        );
        assert_eq!(
            needs_upgrade("Access Denied. You are blocked."),
            Some("wall-word")
        );
        // 少于 20 词
        assert_eq!(needs_upgrade("short page"), Some("thin-content"));
        // 健康正文（20 词以上、无墙词）不升级
        let healthy = "word ".repeat(25);
        assert_eq!(needs_upgrade(&healthy), None);
    }

    /// HTML 粗抽：剥 script/style 与标签、压空白、保留正文词序。
    #[test]
    fn strip_html_basic() {
        let html = "<html><head><title>T</title><style>.a{}</style></head>\
                    <body><script>var x=1;</script><h1>Head</h1><p>Body text here</p></body></html>";
        let t = strip_html(html);
        assert_eq!(t, "HeadBody text here");
    }
}
