//! 无头引擎登录态按域克隆（#48）：从附着浏览器热迁指定域 cookie 到
//! 临时实例。只读源、绝不写回（用户浏览器零改动，铁律）；全量导出
//! 过宽，按域过滤最小化。

use anyhow::{Result, anyhow, bail};
use cdp::Session;
use serde_json::{Value, json};

/// 从附着浏览器克隆指定域 cookie 到目标引擎会话。
///
/// 源发现走附着探测（9222 / DevToolsActivePort；无附着浏览器即报错带
/// CTA）。域匹配是后缀（example.com 命中 .example.com 与 sub.example.com）。
/// 回执带迁移条数与跳过条数。
///
/// # Errors
///
/// 无附着浏览器；源连接失败；目标 setCookies 失败。
pub async fn clone_domains(dst: &Session, domains: &[String]) -> Result<Value> {
    if domains.is_empty() {
        bail!("clone_domains 缺域；下一步：up --headless --cookies example.com[,b.com]");
    }
    let ws = cdp::discovery::probe_default()
        .await
        .ok_or_else(|| anyhow!(
            "没有可附着的源浏览器（--cookies 的克隆源）；下一步：先开日常浏览器带调试口，或用 exportStorageState/importStorageState 整包往返"
        ))?;
    let src = Session::new();
    src.connect(&ws).await?;
    // 源侧补 attach 加 enable（评审 F1：getAllCookies 虽是浏览器级，裸连
    // 接无附着 session 时 100% 失败）；复用 attach_first_page 的写法
    let tabs = src.list_page_targets().await?;
    let first = tabs
        .iter()
        .find(|t| t.type_ == "page")
        .ok_or_else(|| anyhow!("源浏览器没有可附着的 page tab；下一步：开一个普通页面再试"))?;
    src.use_target(&first.target_id).await?;
    let _ = src.call("Network.enable", json!({})).await;
    let all = src.call("Network.getAllCookies", json!({})).await?;
    let cookies = all
        .get("cookies")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut picked: Vec<Value> = Vec::new();
    // DNS 域不区分大小写（评审 G2）：两侧都归一小写；输入前导点剥掉
    let want: Vec<String> = domains
        .iter()
        .map(|d| d.trim().trim_start_matches('.').to_ascii_lowercase())
        .collect();
    for c in cookies {
        let dom = c
            .get("domain")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_ascii_lowercase();
        let bare = dom.trim_start_matches('.');
        if want
            .iter()
            .any(|d| bare == d || bare.ends_with(&format!(".{d}")))
        {
            picked.push(c);
        }
    }
    let total = picked.len();
    // 只读源：连接即断，不写回任何键
    drop(src);
    let mut ok = 0usize;
    let mut failed = 0usize;
    let mut first_err: Option<String> = None;
    for c in &picked {
        let mut p = json!({});
        for k in [
            "name", "value", "domain", "path", "secure", "httpOnly", "sameSite", "expires",
        ] {
            if let Some(v) = c.get(k) {
                p[k] = v.clone();
            }
        }
        // 失败可见（评审 G1）：cloned 小于 matched 时给 failed 计数与首错
        match dst.call("Network.setCookie", p).await {
            Ok(_) => ok += 1,
            Err(e) => {
                failed += 1;
                if first_err.is_none() {
                    first_err = Some(format!(
                        "{}: {e:#}",
                        c.get("name").and_then(Value::as_str).unwrap_or("?")
                    ));
                }
            }
        }
    }
    let mut out = json!({
        "cloned": ok,
        "matched": total,
        "failed": failed,
        "domains": domains,
    });
    if let Some(e) = first_err {
        out["firstError"] = json!(e);
    }
    Ok(out)
}
