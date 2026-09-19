//! issue 通道客户端（REQ-057 契约，issues.ohmygh.com）：缺陷一键反馈。
//!
//! 提交自动署名 `tool=browse` 加版本（编译期 Cargo 版）加平台加主机名，
//! 客户端先做与 Worker 同形的校验与截断（title 1 至 200、body 至多 20000、
//! version 40、platform 与 host 64）；读面 list 与 show 走 GET。
//! `BROWSE_ISSUES_API` 覆写基址（测与灰度，同 omc 的 OMC_ISSUES_API 惯例）。

use anyhow::{Result, bail};
use serde_json::{Value, json};

/// issue 服务缺省基址（Worker 加 D1 真源，REQ-057）。
pub const ISSUES_API: &str = "https://issues.ohmygh.com";

/// 工具署名（契约形 `^[a-z][a-z0-9_-]{0,31}$`，browse 合法）。
const TOOL: &str = "browse";

/// 基址解析（env 覆写，空值忽略，尾斜杠剥掉）。
fn api_base() -> String {
    std::env::var("BROWSE_ISSUES_API")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| ISSUES_API.to_string())
        .trim_end_matches('/')
        .to_string()
}

/// 按 UTF-8 字符数截断（契约上限）。
fn truncate(s: String, n: usize) -> String {
    s.chars().take(n).collect()
}

/// 运行平台形 `<os>-<arch>`（如 `linux-x86_64`）。
fn platform() -> String {
    truncate(
        format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
        64,
    )
}

/// 主机名（unix 读 /etc/hostname，windows 读 COMPUTERNAME；取不到留空）。
fn host() -> String {
    let raw = if cfg!(windows) {
        std::env::var("COMPUTERNAME").unwrap_or_default()
    } else {
        std::fs::read_to_string("/etc/hostname")
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    };
    truncate(raw, 64)
}

/// 预览一条 issue 载荷（#57 G6 `--dry-run`）：与 [`new`] 同规校验，不发
/// 网络请求，回应发载荷（含自动署名字段）。契约实弹与演练走这里，不再
/// 往生产台账落测试单（评审首单实弹 -b 契约真发了 #56 的教训）。
///
/// # Errors
///
/// 客户端校验不过（title 空或超 200、body 超 20000），与 [`new`] 同文。
pub fn dry_run(title: &str, body: &str) -> Result<Value> {
    let title = title.trim();
    if title.is_empty() || title.chars().count() > 200 {
        bail!("title 长度要在 1 至 200（trim 后）；下一步：改标题再提");
    }
    if body.chars().count() > 20000 {
        bail!("body 至多 20000 字符；下一步：精简正文或分段提交");
    }
    Ok(json!({
        "tool": TOOL,
        "title": title,
        "body": body,
        "version": truncate(env!("CARGO_PKG_VERSION").to_string(), 40),
        "platform": platform(),
        "host": host(),
    }))
}

async fn http() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| anyhow::anyhow!("构建 http client 失败：{e}"))
}

/// 提交一条 issue：`POST /api/issues`，回执 `{ok, id, url}`（url 即详情页）。
///
/// # Errors
///
/// 客户端校验不过（title 空或超 200、body 超 20000）；429 限速（每 IP
/// 每时 10 条）；400 服务端校验；网络或超时。
pub async fn new(title: &str, body: &str) -> Result<Value> {
    let title = title.trim();
    if title.is_empty() || title.chars().count() > 200 {
        bail!("title 长度要在 1 至 200（trim 后）；下一步：改标题再提");
    }
    if body.chars().count() > 20000 {
        bail!("body 至多 20000 字符；下一步：精简正文或分段提交");
    }
    let payload = json!({
        "tool": TOOL,
        "title": title,
        "body": body,
        "version": truncate(env!("CARGO_PKG_VERSION").to_string(), 40),
        "platform": platform(),
        "host": host(),
    });
    let resp = http()
        .await?
        .post(format!("{}/api/issues", api_base()))
        .json(&payload)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("issue 提交失败（{}）：{e}", api_base()))?;
    match resp.status().as_u16() {
        201 => resp
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("回执解析失败：{e}")),
        429 => bail!("限速（每 IP 每时 10 条）；下一步：整点后再提，或去网页面看现有条目"),
        400 => bail!("服务端校验不过（400）；下一步：核对 title 与 body 长度"),
        code => bail!("issue 服务回 {code}；下一步：稍后重试，持续失败带此码反馈"),
    }
}

/// 列 issue：`GET /api/issues?tool=&status=&limit=&before=`（新到旧，limit 1
/// 至 100 由本函数 clamp 到界；`before` 是 keyset 游标（#53），取该 id 之前
/// 更早的一页，非法值服务端回 400）。返回 `count` 是本次返回条数非在册
/// 总数；返回条数恰打满 limit 时 stderr 出一行截断提示（#52）。
///
/// # Errors
///
/// 网络或超时；服务端非 200（含 before 非法 400）。
pub async fn list(
    tool: Option<&str>,
    status: Option<&str>,
    limit: u32,
    before: Option<&str>,
) -> Result<Value> {
    let limit = limit.clamp(1, 100);
    let mut url = format!("{}/api/issues?limit={limit}", api_base());
    let tool = tool.unwrap_or(TOOL);
    url.push_str(&format!("&tool={}", urlencode(tool)));
    if let Some(s) = status {
        url.push_str(&format!("&status={}", urlencode(s)));
    }
    if let Some(b) = before {
        url.push_str(&format!("&before={}", urlencode(b)));
    }
    let resp = http()
        .await?
        .get(url)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("issue 列表拉取失败：{e}"))?;
    if !resp.status().is_success() {
        // 服务端 error 字段归因不丢（评审 F3）；4xx 是请求面问题（如
        // --before 非正整数），指查参数而非重试
        let code = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        let err = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_string))
            .unwrap_or_else(|| truncate(text, 200));
        if (400..500).contains(&code) {
            bail!(
                "issue 服务回 {code}：{err}；下一步：核对 --before/--status/--tool/--limit 取值（4xx 是请求面问题，重试无用）"
            );
        }
        bail!("issue 服务回 {code}：{err}；下一步：稍后重试");
    }
    let v: Value = resp
        .json()
        .await
        .map_err(|e| anyhow::anyhow!("列表解析失败：{e}"))?;
    // 饱和即或被截断（count 只报返回数，#52）：打满 limit 时旧条目可能仍
    // 不可见，指路收窄与翻页
    if v["count"].as_u64() == Some(u64::from(limit)) {
        eprintln!(
            "[browse] issue list 恰返回 {limit} 条（=limit，或被截断）；下一步：--status/--tool 过滤收窄，或 --before <id> 翻更早一页"
        );
    }
    Ok(v)
}

/// 看 issue 详情：`GET /api/issues/<id>`。
///
/// # Errors
///
/// id 不存在（404）；网络或超时。
pub async fn show(id: &str) -> Result<Value> {
    let url = format!("{}/api/issues/{}", api_base(), urlencode(id));
    let resp = http()
        .await?
        .get(url)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("issue 详情拉取失败：{e}"))?;
    match resp.status().as_u16() {
        200 => resp
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("详情解析失败：{e}")),
        404 => bail!("issue {id} 不存在；下一步：browse issue list 核对 id"),
        code => bail!("issue 服务回 {code}；下一步：稍后重试"),
    }
}

/// 极简百分号编码（query 用：tool/status/id 都是受控字符集，留安全网）。
fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
