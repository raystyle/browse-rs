//! 连接线索到 WebSocket URL 的解析：`wsUrl` / `port` / `profileDir` 三条路。
//!
//! Chrome 144+ 对默认 profile 的 HTTP `/json` 端点可能不服务，
//! `DevToolsActivePort` 文件（user-data 目录根下，首行端口、次行 WS 路径）
//! 是可靠兜底。clean-chrome 无参数启动即开 9222，正常走 `/json/version`。

use anyhow::{Context, Result, anyhow};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// 端口字面量的宽容解析：`"9222"`、`"http://127.0.0.1:9222"`、`"127.0.0.1:9222/"` 都出 `9222`。
///
/// # Examples
///
/// ```
/// assert_eq!(cdp::parse_port("9222"), Some(9222));
/// assert_eq!(cdp::parse_port("http://127.0.0.1:9222"), Some(9222));
/// assert_eq!(cdp::parse_port("not-a-port"), None);
/// ```
pub fn parse_port(s: &str) -> Option<u16> {
    if let Ok(p) = s.parse::<u16>() {
        return Some(p);
    }
    let s = s
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    if let Some((_, port)) = s.rsplit_once(':') {
        let port = port.trim_end_matches('/');
        return port.parse().ok();
    }
    None
}

/// 从 `DevToolsActivePort` 文件文本解析 WS URL：首行端口，次行 `/devtools/...` 路径。
///
/// # Examples
///
/// ```
/// let text = "52345\n/devtools/browser/8f4eaaaa-1111\n";
/// assert_eq!(
///     cdp::ws_from_active_port_text(text),
///     Some("ws://127.0.0.1:52345/devtools/browser/8f4eaaaa-1111".to_string()),
/// );
/// assert_eq!(cdp::ws_from_active_port_text("garbage"), None);
/// ```
pub fn ws_from_active_port_text(text: &str) -> Option<String> {
    let mut lines = text.lines();
    let port = lines.next()?.trim();
    let path = lines.next()?.trim();
    if path.starts_with("/devtools/") {
        Some(format!("ws://127.0.0.1:{port}{path}"))
    } else {
        None
    }
}

/// 把 [`super::ConnectOptions`] 三线索解析成 WS URL。
///
/// `profileDir` 路径会轮询等文件出现（Chrome 启动到写文件有窗口期），
/// 其余立即解析；整体超时由 [`super::ConnectOptions::timeout_ms`] 控制
/// （缺省 5 秒）。
///
/// # Errors
///
/// - `wsUrl` 是 http 端点但 `/json/version` 请求失败或没有 `webSocketDebuggerUrl`。
/// - `profileDir` 超时内读不到合法的 `DevToolsActivePort`。
/// - 三线索全空时按默认端口 9222 走 `/json/version`，失败同上。
pub async fn resolve_ws_url(opts: &super::ConnectOptions) -> Result<String> {
    let dur = Duration::from_millis(opts.timeout_ms.unwrap_or(5000));
    if let Some(u) = &opts.ws_url {
        if u.starts_with("ws") {
            return Ok(u.clone());
        }
        return http_version_ws_url(u, dur).await;
    }
    if let Some(dir) = &opts.profile_dir {
        return wait_active_port_file(Path::new(dir), dur).await;
    }
    let port = opts.port.unwrap_or(9222);
    http_version_ws_url(&format!("http://127.0.0.1:{port}"), dur).await
}

/// GET `<http>/json/version` 取 `webSocketDebuggerUrl`（自带 `timeout` 超时）。
///
/// # Errors
///
/// 请求失败、非 JSON、或缺 `webSocketDebuggerUrl` 字段。
pub async fn http_version_ws_url(http: &str, timeout: Duration) -> Result<String> {
    let http = http.trim_end_matches('/');
    let url = format!("{http}/json/version");
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .context("build http client")?;
    let body: Value = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?
        .json()
        .await
        .with_context(|| format!("parse {url}"))?;
    body.get("webSocketDebuggerUrl")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| anyhow!("no webSocketDebuggerUrl in {url}"))
}

/// 轮询 profile 目录下的 `DevToolsActivePort`，直到超时。
///
/// # Errors
///
/// `timeout` 内文件没出现或内容不合法。
pub async fn wait_active_port_file(profile_dir: &Path, timeout: Duration) -> Result<String> {
    let path = profile_dir.join("DevToolsActivePort");
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if let Ok(text) = tokio::fs::read_to_string(&path).await
            && let Some(ws) = ws_from_active_port_text(&text)
        {
            return Ok(ws);
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(anyhow!("could not read {}", path.display()));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// 返回默认 user-data 目录清单（clean-chrome / Chromium / Chrome，按探测优先序）。
///
/// clean-chrome 是 Chromium 品牌，profile 在 `%LOCALAPPDATA%\Chromium\User Data`。
pub fn default_profile_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let local = PathBuf::from(local);
        dirs.push(local.join("Chromium").join("User Data"));
        dirs.push(local.join("Google").join("Chrome").join("User Data"));
    }
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        let home = PathBuf::from(home);
        dirs.push(home.join(".config").join("chromium"));
    }
    dirs
}

/// 探测本机已开调试口的浏览器，返回其 WS URL；没有则 `None`。
///
/// 顺序：默认端口 9222 的 `/json/version`（300ms 短超时，别让缺浏览器拖慢冷启动）
/// -> 各默认 profile 目录下的 `DevToolsActivePort` 文件（静态读一次，
/// 浏览器已在跑则文件已在）。
pub async fn probe_default() -> Option<String> {
    let fast = reqwest::Client::builder()
        .timeout(Duration::from_millis(300))
        .build()
        .ok()?;
    if let Ok(resp) = fast.get("http://127.0.0.1:9222/json/version").send().await
        && let Ok(body) = resp.json::<Value>().await
        && let Some(ws) = body.get("webSocketDebuggerUrl").and_then(|v| v.as_str())
    {
        return Some(ws.to_string());
    }
    for dir in default_profile_dirs() {
        if let Ok(text) = std::fs::read_to_string(dir.join("DevToolsActivePort"))
            && let Some(ws) = ws_from_active_port_text(&text)
        {
            return Some(ws);
        }
    }
    None
}

/// 可附着浏览器的候选描述，由 [`detect_browsers`] 产出。
#[derive(Debug, Clone, serde::Serialize)]
pub struct DetectedBrowser {
    /// user-data 目录（候选来源）。
    pub profile_dir: PathBuf,
    /// `DevToolsActivePort` 首行端口。
    pub port: u16,
    /// 可直连的 browser 级 WS URL。
    pub ws_url: String,
    /// 端口文件 mtime 毫秒（候选排序依据：越新越可能是在跑的那个）。
    pub mtime_ms: u128,
}

/// 扫默认 profile 目录列出所有可附着候选：读各目录的 `DevToolsActivePort`
/// 按 mtime 降序（最近启动优先），同步、零网络，对齐官方 harness 的
/// `detectBrowsers()`。
///
/// # Examples
///
/// ```no_run
/// # // no_run：扫的是本机真实 profile 目录，结果随机器而变，不可断言
/// # async fn demo() {
/// for b in cdp::discovery::detect_browsers() {
///     println!("{} port={} mtime={}", b.profile_dir.display(), b.port, b.mtime_ms);
/// }
/// # }
/// ```
pub fn detect_browsers() -> Vec<DetectedBrowser> {
    let mut hits = Vec::new();
    for dir in default_profile_dirs() {
        let path = dir.join("DevToolsActivePort");
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Some(ws) = ws_from_active_port_text(&text) else {
            continue;
        };
        let port = text
            .lines()
            .next()
            .and_then(|l| l.trim().parse::<u16>().ok())
            .unwrap_or(0);
        hits.push(DetectedBrowser {
            profile_dir: dir,
            port,
            ws_url: ws,
            mtime_ms: meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis())
                .unwrap_or(0),
        });
    }
    hits.sort_by_key(|b| std::cmp::Reverse(b.mtime_ms));
    hits
}
