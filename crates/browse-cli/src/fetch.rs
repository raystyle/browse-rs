//! 一次性只读抓取（#50/#56）：HTTP 优先，三条件升级引擎，markdown 直出。
//!
//! - HTTP 腿：reqwest 直取（零浏览器成本），判升级三条件：正文空、命中
//!   墙词、正文少于 20 词。
//! - 引擎腿：经 daemon POST /eval 走 goto 加页内抽取（启发式标题加段落，
//!   非 Readability；v1 口径已在 surface 披露）。
//! - 域名层点名（#56）：两腿回执都按 URL 做 goto 同口径匹配，命中
//!   workspace 站点知识附 domain_skills 与 hint；不依赖引擎（引擎腿
//!   daemon 侧 goto 的点名回执被抽取串替换，CLI 侧补点）。

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
        None => {
            let mut r = json!({
                "url": url, "via": "http", "status": status,
                "title": title,
                "text": text,
                "bytes": html.len(),
            });
            add_domain_skills(url, &mut r);
            Ok(r)
        }
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
                    // CTA 指日志（#49 评审随记下批项）：daemon 在跑却不可达
                    // 多为重启窗口或端口不符，日志比「browse up」更对症
                    anyhow!(
                        "引擎腿 daemon 不可达（{e}）；下一步：看日志 {}（tail）或 browse status 对端口",
                        crate::client::state_dir().join("daemon.log").display()
                    )
                })?;
            let v: Value = resp.json().await?;
            // #60：引擎腿信封的引擎上下文告警（与 eval 道同款渲染）
            if let Some(ctx) = v.get("engineContext") {
                crate::client::render_engine_context(ctx);
            }
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
            let mut r = json!({
                "url": url, "via": "engine", "upgradedFrom": why, "httpStatus": status,
                "title": inner.get("title"),
                "text": inner.get("text"),
            });
            add_domain_skills(url, &mut r);
            Ok(r)
        }
    }
}

/// 域名层点名（#56，两腿共用缝）：URL 与 workspace 目录取
/// [`browse_core::skills::url_domain_fields`]，未命中或关闭零插入。
fn add_domain_skills(url: &str, r: &mut Value) {
    for (k, v) in browse_core::skills::url_domain_fields(&browse_core::paths::workspace_dir(), url)
    {
        if let Some(obj) = r.as_object_mut() {
            obj.insert(k.to_string(), v);
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

    /// #56 域名点名两腿接线：命中时两腿回执形都附 domain_skills 与
    /// hint；未命中（data: URL 无域名段）键集与现状逐字节一致（G4 钉
    /// 进回归网的验收第一条）。
    #[test]
    fn domain_skills_wiring_both_legs() {
        let _env_guard = crate::TEST_ENV_LOCK.lock().unwrap();
        let ws = std::env::temp_dir().join(format!("browse-fetch-sk56-{}", std::process::id()));
        let seg = ws.join("domain-skills").join("x");
        std::fs::create_dir_all(&seg).unwrap();
        std::fs::write(seg.join("notes.md"), "# x").unwrap();
        let saved = std::env::var_os("BROWSE_WORKSPACE");
        // SAFETY: TEST_ENV_LOCK 窗内独占改 env，块尾按原值还原
        unsafe { std::env::set_var("BROWSE_WORKSPACE", &ws) };
        // HTTP 腿回执形：命中附两键
        let mut http = json!({
            "url": "https://www.x.com/a", "via": "http", "status": 200,
            "title": "t", "text": "body", "bytes": 42,
        });
        add_domain_skills("https://www.x.com/a", &mut http);
        assert_eq!(
            http.get("domain_skills"),
            Some(&json!(["notes.md"])),
            "{http}"
        );
        assert!(
            http.get("domain_skills_hint")
                .and_then(Value::as_str)
                .is_some_and(|h| h == "browse workspace site x"),
            "{http}"
        );
        // 引擎腿回执形：同口径命中
        let mut engine = json!({
            "url": "https://www.x.com/a", "via": "engine",
            "upgradedFrom": "thin-content", "httpStatus": 200,
            "title": "t", "text": "body",
        });
        add_domain_skills("https://www.x.com/a", &mut engine);
        assert_eq!(
            engine.get("domain_skills"),
            Some(&json!(["notes.md"])),
            "{engine}"
        );
        // 未命中：键集恰为现状六键（逐字节一致的等价断言，BTreeMap 键序）
        let mut miss = json!({
            "url": "data:text/html,x", "via": "http", "status": 200,
            "title": "t", "text": "body", "bytes": 42,
        });
        add_domain_skills("data:text/html,x", &mut miss);
        let mut keys: Vec<&str> = miss
            .as_object()
            .expect("对象回执")
            .keys()
            .map(|k| k.as_str())
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec!["bytes", "status", "text", "title", "url", "via"],
            "未命中零新增键: {miss}"
        );
        // SAFETY: 同一锁窗内按原值还原
        unsafe {
            match &saved {
                Some(v) => std::env::set_var("BROWSE_WORKSPACE", v),
                None => std::env::remove_var("BROWSE_WORKSPACE"),
            }
        }
        let _ = std::fs::remove_dir_all(&ws);
    }
}
