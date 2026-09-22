//! browse 自更新（用户令 2026-09-18；对齐 build-release 公共契约第六节
//! 双通道）：镜像 stable/latest 判新（GitHub 回落）（semver 只升不降）-> 下载本平台
//! 资产（自家镜像 stable 滚动段优先，GitHub 回落，资产与边车恒同源）->
//! `.sha256` 边车锚校验（与发布器同 digest 判据，不符即拒不回落）->
//! 解包取二进制 -> 原子自替换（同目录暂存防跨文件系统 rename，pid 后缀
//! 防并发互踩，`--version` 自证带重试，证败回滚并复核终态）。
//!
//! ark 管理的安装拦自更新走 ark 单通道（判据：exe 同目录 `ark-managed`
//! 落痕，或用户面 bin 目录存在指向本 exe 的符号链接入口即 ark 布局；
//! 落痕生产者契约随家族统一标准派 ark 侧）。

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// 镜像基址常量。
///
/// stable 滚动段路由 `<base>/stable/<资产>`；`BROWSE_RELEASE_MIRROR` 覆写
/// 走测试或自建镜像。
pub const RELEASE_MIRROR: &str = "https://browse.ohmygh.com/browse";

/// GitHub 仓（latest API 判新与回落下载）。
pub const GITHUB_REPO: &str = "raystyle/browse_rs";

/// 本平台资产三元组（与本仓发布流水一致：win 是 gnu 非 msvc）。
/// aarch64/musl Linux 无发布资产，调用方以 [`asset_name`] 的错误 CTA 兜底。
fn asset_triple() -> Option<&'static str> {
    if cfg!(target_os = "windows") {
        Some("x86_64-pc-windows-gnu")
    } else if cfg!(target_os = "macos") {
        Some("aarch64-apple-darwin")
    } else if cfg!(all(
        target_os = "linux",
        target_arch = "x86_64",
        target_env = "gnu"
    )) {
        Some("x86_64-unknown-linux-gnu")
    } else {
        None
    }
}

/// 某版本的本平台资产名（`v` 前缀形，与 release 六件命名一致）。
///
/// # Errors
///
/// 本平台无发布资产（aarch64/musl Linux）：错误带源码安装 CTA。
pub fn asset_name(version: &str) -> Result<String> {
    let Some(triple) = asset_triple() else {
        bail!(
            "本平台无发布资产（aarch64/musl Linux 未出包）；\
             下一步：cargo install --path crates/browse-cli 从源码装"
        );
    };
    let ext = if cfg!(target_os = "windows") {
        "zip"
    } else {
        "tar.gz"
    };
    Ok(format!("browse-v{version}-{triple}.{ext}"))
}

fn mirror_base() -> String {
    std::env::var("BROWSE_RELEASE_MIRROR")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| RELEASE_MIRROR.to_string())
        .trim_end_matches('/')
        .to_string()
}

/// GitHub API 头：有 token（`GH_TOKEN`/`GITHUB_TOKEN`）附 Bearer 提限流
/// （匿名 60/h，机队易撞；带 token 5000/h）。
fn gh_headers() -> Vec<(String, String)> {
    let mut h = vec![("User-Agent".into(), "browse-self-update".into())];
    if let Some(t) = ["GH_TOKEN", "GITHUB_TOKEN"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
    {
        h.push(("Authorization".into(), format!("Bearer {t}")));
    }
    h
}

fn http_get(
    client: &reqwest::blocking::Client,
    url: &str,
    api: bool,
) -> Result<reqwest::blocking::Response> {
    let mut req = client.get(url);
    if api {
        for (k, v) in gh_headers() {
            req = req.header(&k, &v);
        }
    }
    let r = req
        .send()
        .map_err(|e| anyhow::anyhow!("读 {url} 失败（{e}）"))?;
    if api && (r.status() == 403 || r.status() == 429) {
        let ra = r
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("未知");
        bail!(
            "GitHub API 限流（{}，Retry-After {ra}）；下一步：设 GH_TOKEN 提限流（匿名 60/h，带 token 5000/h），或稍候重试",
            r.status()
        );
    }
    let r = r
        .error_for_status()
        .map_err(|e| anyhow::anyhow!("读 {url} 失败（{e}）"))?;
    Ok(r)
}

/// 点分数版本比较（`0.7.0`、`0.7.0-r2` 形；数值段逐位比，后缀按字典序
/// 兜底）。返回 candidate 是否比 current 新。
fn version_newer(candidate: &str, current: &str) -> bool {
    let parse = |v: &str| -> (Vec<u64>, String) {
        let (nums, suf) = match v.split_once('-') {
            Some((n, s)) => (n, s.to_string()),
            None => (v, String::new()),
        };
        (
            nums.split('.')
                .map(|p| p.parse::<u64>().unwrap_or(0))
                .collect(),
            suf,
        )
    };
    let (a, asuf) = parse(candidate);
    let (b, bsuf) = parse(current);
    if a != b {
        return a > b;
    }
    asuf > bsuf
}

/// 自更新 HTTP 客户端（镜像域恒 HTTP/1.1，总台令 2026-09-23）：镜像前置链
/// （openwrt 透明代理）对 HTTP/2 存在约 1,720,320B 的字节悬崖（h2 精确断流；
/// 强制 h1 同路径 1.7MB/s 全通），故判新腿与下载腿统一钉死 h1。本仓 reqwest
/// 现为 http1-only 编译面（workspace 关 `default-features`、未开 `http2`
/// feature，`h2` 不在依赖图），显式 `.http1_only()` 是防漂移口径：日后任一
/// 依赖经 feature 统一化打开 `reqwest/http2` 时，镜像腿仍钉在 h1，不随全局
/// 协商漂回 h2。
///
/// # Errors
///
/// client 构建失败（TLS 后端初始化等）。
fn update_http_client(timeout: std::time::Duration) -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .http1_only()
        .connect_timeout(std::time::Duration::from_secs(30))
        .timeout(timeout)
        .build()
        .map_err(|e| anyhow::anyhow!("构建 http client 失败：{e}"))
}

/// 发现最新版本号。
///
/// 镜像 stable 段的 `latest` 标记优先（播种流水写的单行纯版本号），
/// 镜像不可达、标记缺失或不成形才回落 GitHub Releases latest API
/// （tag 去 `v` 前缀）；GitHub 匿名 60/h 机队易撞（#54），镜像面让日
/// 常判新与下载腿同源，update 全程默认零 GitHub 依赖。
///
/// 阻塞 http，调用方收 `spawn_blocking`。
///
/// # Errors
///
/// 双源皆失败（含 GitHub 限流 403/429 带 token 指引）或都未给出成形
/// 版本号。
pub fn latest_browse_version() -> Result<String> {
    let client = update_http_client(std::time::Duration::from_secs(60))?;
    // 镜像 latest 优先：任何不成形（404、超时、垃圾文本）静默回落
    // GitHub，镜像故障不放大成更新失败
    let latest_url = format!("{}/stable/latest", mirror_base());
    if let Ok(r) = http_get(&client, &latest_url, false)
        && let Ok(text) = r.text()
        && let Some(v) = parse_latest_line(&text)
    {
        eprintln!("browse update：判新走镜像 stable/latest（{v}）");
        return Ok(v);
    }
    eprintln!("browse update：镜像 latest 未命中，判新回落 GitHub API");
    let api = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest");
    let body: Value = http_get(&client, &api, true)?
        .json()
        .map_err(|e| anyhow::anyhow!("解析 GitHub latest JSON 失败：{e}"))?;
    let tag = body
        .get("tag_name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim_start_matches('v')
        .to_string();
    if tag.is_empty() {
        bail!(
            "GitHub latest API 未回 tag_name；下一步：手动升级走镜像 stable 段或 \
             GitHub Releases，或 BROWSE_RELEASE_MIRROR 换源"
        );
    }
    Ok(tag)
}

/// 从镜像 `latest` 标记文本提取纯版本号：去空白后恰是三段 ASCII 数字
/// （播种流水写的单行形）；任何其他形态（HTML 错误页、垃圾文本、空）
/// 返 `None` 交回落，镜像面损坏不误判版本。
fn parse_latest_line(text: &str) -> Option<String> {
    let t = text.trim();
    let well = !t.is_empty()
        && t.len() <= 16
        && t.split('.').count() == 3
        && !t.starts_with('.')
        && !t.ends_with('.')
        && !t.contains("..")
        && t.bytes().all(|b| b.is_ascii_digit() || b == b'.');
    well.then(|| t.to_string())
}

/// 从一个源取「边车 + 资产」对（边车先行省流量）；任何一步失败即该源
/// 整对作废（防资产与边车混源）。
fn fetch_pair(
    client: &reqwest::blocking::Client,
    base: &str,
    asset: &str,
    api: bool,
    progress: &mut dyn FnMut(u64, Option<u64>),
    on_stall: std::sync::Arc<std::sync::Mutex<dyn FnMut(u64, u64) + Send>>,
) -> Result<(Vec<u8>, String)> {
    let sidecar_url = format!("{base}/{asset}.sha256");
    let sidecar = http_get(client, &sidecar_url, api)?
        .text()
        .map_err(|e| anyhow::anyhow!("读边车 body 失败：{e}"))?;
    let want = sidecar.split_whitespace().next().unwrap_or("");
    if want.len() != 64 || !want.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("边车 {sidecar_url} 内容非法（要 64 位十六进制）");
    }
    let resp = http_get(client, &format!("{base}/{asset}"), api)?;
    let bytes = read_body_with_progress(
        resp,
        asset,
        &mut ProgressHooks {
            min_bytes: PROGRESS_MIN_BYTES,
            min_interval: PROGRESS_MIN_INTERVAL,
            stall_interval: STALL_WARN_INTERVAL,
            tick: STALL_WATCH_TICK,
            on_progress: progress,
            on_stall,
        },
    )?;
    Ok((bytes, want.to_string()))
}

/// CLI 面的下载心跳节流（#58）：收满此字节数或距上次满此时长即打一行，
/// 慢源下「在下」与「死」可辨。快源被字节闸限到每 512KB 一行（4MB 包
/// 约八行），慢源被时长闸托底到每秒一行。
const PROGRESS_MIN_BYTES: u64 = 512 * 1024;
const PROGRESS_MIN_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

/// 流式读 body 加进度回调（#58 加 G1，crate 内缝）：逐块读（64KB 块），
/// 过节流闸（`min_bytes` 或 `min_interval` 任一满足）且未到总量时回调
/// `on_progress(已收, Option<总量>)`；读失败（超时、断流）的错误带
/// 进度上下文（已收/总量/耗时），超时行为可预期；零字节停顿每满
/// `stall_interval` 经独立看门狗线程回调 `on_stall(已收, 停顿秒)`（节拍
/// `tick`，读结束后看门狗最多再活一个节拍即退，不 join 不阻塞收尾）。
/// 何时用：下载腿的资产体（边车与判新标记是单行小件不走此道）。
/// 边界：Content-Length 缺失时总量为 `None`（两闸仍都参与，完成前最后
/// 一块可能多拍一拍「下载中」，现网镜像恒带 CL 无实害）；完成态在有
/// CL 时不回调（收尾行归调用方）；预分配对声明的 CL 封顶 64MB（G3：
/// BROWSE_RELEASE_MIRROR 可指向任意源，坏源巨型声明不得触发巨量分配）。
/// 进度与 stall 回显的参数束（#58 加 G1，crate 内缝）：节流闸、看门狗
/// 闸与节拍、两路回调收进一束，防 [`read_body_with_progress`] 参数面
/// 膨胀（评审 clippy too_many_arguments）。
pub(crate) struct ProgressHooks<'a> {
    /// 心跳字节闸：距上次回调增量满此值即拍。
    pub(crate) min_bytes: u64,
    /// 心跳时长闸：距上次回调满此时长即拍。
    pub(crate) min_interval: std::time::Duration,
    /// stall 告警闸：零字节停顿满此时长告警一次。
    pub(crate) stall_interval: std::time::Duration,
    /// 看门狗轮询节拍。
    pub(crate) tick: std::time::Duration,
    /// 心跳回调（已收，Option<总量>）。
    pub(crate) on_progress: &'a mut dyn FnMut(u64, Option<u64>),
    /// stall 告警回调（已收，停顿秒；看门狗线程侧调，故 Arc<Mutex>）。
    pub(crate) on_stall: std::sync::Arc<std::sync::Mutex<dyn FnMut(u64, u64) + Send>>,
}

fn read_body_with_progress(
    resp: reqwest::blocking::Response,
    asset: &str,
    hooks: &mut ProgressHooks<'_>,
) -> Result<Vec<u8>> {
    let total = resp
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    // G3：预分配封顶（资产实为 MB 级，64MB 帽只挡恶意声明；评审附言
    // 从 256MB 收紧）
    const ALLOC_CAP: u64 = 64 * 1024 * 1024;
    let mut body = Vec::with_capacity(total.unwrap_or(0).min(ALLOC_CAP) as usize);
    stream_with_progress(
        resp,
        asset,
        "重试一次，仍失败 BROWSE_RELEASE_MIRROR 换源",
        hooks,
        &mut |chunk| {
            body.extend_from_slice(chunk);
            Ok(())
        },
    )?;
    Ok(body)
}

/// 通用流式读加进度与看门狗（crate 内缝，#58 加 G1）：逐块读（64KB 块）
/// 过 `sink` 消费（update 腿收 Vec、chrome 腿写文件加哈希，共用同一
/// 心跳/看门狗/错误上下文口径），返回总字节数。读失败（超时、断流）
/// 的错误带 `label`（资产名）、已收/总量/耗时加 `next_step`（调用方
/// CTA）。G1 看门狗：零字节停顿每满 `hooks.stall_interval` 经独立线程
/// 回调（节拍 `hooks.tick`，读结束后最多再活一个节拍即退，不 join 不
/// 阻塞收尾）。边界：Content-Length 缺失时总量为 `None`（两闸仍都参
/// 与，完成前最后一块可能多拍一拍心跳，诚实源恒带 CL 无实害）；完成
/// 态在有 CL 时不回调（收尾行归调用方）；推进时刻在 read 返回后即刷，
/// sink 落盘耗时不算停顿（评审 G3）。
pub(crate) fn stream_with_progress(
    resp: reqwest::blocking::Response,
    label: &str,
    next_step: &str,
    hooks: &mut ProgressHooks<'_>,
    sink: &mut dyn FnMut(&[u8]) -> std::io::Result<()>,
) -> Result<u64> {
    use std::io::Read;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    let total = resp
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    let mut resp = resp;
    let started = std::time::Instant::now();
    let mut last_emit = std::time::Instant::now();
    let mut last_bytes: u64 = 0;
    let mut received: u64 = 0;
    let mut buf = [0u8; 64 * 1024];
    // G1 看门狗共享态：字节计数与最近推进时刻（读侧 n>0 即刷）；看门狗
    // 分离线程按 tick 轮询，零字节停顿满 stall_interval 回调 on_stall，
    // 读结束置 done 后最多再活一个 tick 即退（不 join 不阻塞收尾）
    let bytes = std::sync::Arc::new(AtomicU64::new(0));
    let advanced_at = std::sync::Arc::new(std::sync::Mutex::new(std::time::Instant::now()));
    let done = std::sync::Arc::new(AtomicBool::new(false));
    {
        let bytes = bytes.clone();
        let advanced_at = advanced_at.clone();
        let done = done.clone();
        let tick = hooks.tick;
        let stall_interval = hooks.stall_interval;
        let on_stall = hooks.on_stall.clone();
        std::thread::spawn(move || {
            let mut last_warn: Option<std::time::Instant> = None;
            loop {
                std::thread::sleep(tick);
                if done.load(Ordering::Relaxed) {
                    return;
                }
                let stalled = advanced_at.lock().map(|t| t.elapsed()).ok();
                if let Some(s) = stalled
                    && s >= stall_interval
                    && last_warn.is_none_or(|w| w.elapsed() >= stall_interval)
                {
                    on_stall
                        .lock()
                        .map(|mut f| f(bytes.load(Ordering::Relaxed), s.as_secs()))
                        .ok();
                    last_warn = Some(std::time::Instant::now());
                }
            }
        });
    }
    let result = loop {
        let step = match resp.read(&mut buf) {
            Ok(n) => n,
            Err(e) => {
                let elapsed = started.elapsed().as_secs();
                break Err(anyhow::anyhow!(
                    "读 {label} 失败（{e}）：已收 {}/{}，耗时 {elapsed}s；下一步：{next_step}",
                    received,
                    total
                        .map(|t| t.to_string())
                        .unwrap_or_else(|| "未知".into())
                ));
            }
        };
        if step == 0 {
            break Ok(());
        }
        // G3：推进时刻在 read 返回后即刷（sink 落盘耗时不算停顿，「等
        // 数据」文案只对网络零字节负责）
        received += step as u64;
        bytes.store(received, Ordering::Relaxed);
        if let Ok(mut t) = advanced_at.lock() {
            *t = std::time::Instant::now();
        }
        if let Err(e) = sink(&buf[..step]) {
            break Err(anyhow::anyhow!("落盘 {label} 失败：{e}"));
        }
        let got = received;
        let unread = total.is_none_or(|t| got < t);
        if unread
            && (got - last_bytes >= hooks.min_bytes || last_emit.elapsed() >= hooks.min_interval)
        {
            (hooks.on_progress)(got, total);
            last_emit = std::time::Instant::now();
            last_bytes = got;
        }
    };
    done.store(true, Ordering::Relaxed);
    result.map(|_| received)
}

/// stall 看门狗的告警间隔与轮询节拍（G1，CLI 面）：零字节停顿每满
/// 10 秒告警一次，看门狗每秒看一眼进度态。
pub(crate) const STALL_WARN_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);
pub(crate) const STALL_WATCH_TICK: std::time::Duration = std::time::Duration::from_secs(1);

/// 心跳行文案（#58，纯函数供单测锁形）：`label` 是调用方语境（如
/// `browse update`、`browse chrome install <版本>`）；总量已知给
/// `N/总量（%）`，未知降级字节式。
///
/// # Examples
///
/// ```
/// assert_eq!(
///     browse_core::self_update::heartbeat_line("browse update", 1024, Some(4096)),
///     "browse update：下载中 1024/4096（25%）"
/// );
/// assert_eq!(
///     browse_core::self_update::heartbeat_line("browse chrome install 152", 7, None),
///     "browse chrome install 152：下载中 7 字节（总量未知）"
/// );
/// ```
pub fn heartbeat_line(label: &str, got: u64, total: Option<u64>) -> String {
    match total {
        Some(t) => format!("{label}：下载中 {got}/{t}（{}%）", got * 100 / t.max(1)),
        None => format!("{label}：下载中 {got} 字节（总量未知）"),
    }
}

/// stall 告警行文案（G1，纯函数供单测锁形）：`label` 同 [`heartbeat_line`]。
///
/// # Examples
///
/// ```
/// assert_eq!(
///     browse_core::self_update::stall_line("browse update", 2048, 20),
///     "browse update：仍在等数据（已收 2048 字节，20 秒无进展）"
/// );
/// ```
pub fn stall_line(label: &str, got: u64, stalled_secs: u64) -> String {
    format!("{label}：仍在等数据（已收 {got} 字节，{stalled_secs} 秒无进展）")
}

/// 双通道下载加锚校验：镜像 stable 段整对优先（资产或边车任一 404/失败
/// 即整对回落 GitHub）；哈希不符是安全问题，**硬拒不回落**。
fn fetch_asset(client: &reqwest::blocking::Client, version: &str) -> Result<Vec<u8>> {
    let asset = asset_name(version)?;
    let mirror = format!("{}/stable", mirror_base());
    eprintln!("browse update：镜像 stable 段取 {asset}");
    // #58 心跳面（慢源推进）加 G1 stall 面（零字节停顿）：文案走纯函数
    // （heartbeat_line/stall_line，单测锁形），节流与告警闸见常量
    let mut heartbeat =
        |got: u64, total: Option<u64>| eprintln!("{}", heartbeat_line("browse update", got, total));
    let on_stall: std::sync::Arc<std::sync::Mutex<dyn FnMut(u64, u64) + Send>> =
        std::sync::Arc::new(std::sync::Mutex::new(|got: u64, stalled: u64| {
            eprintln!("{}", stall_line("browse update", got, stalled))
        }));
    // G2（评审）：镜像腿失败诊断透出——断流错误带已收/总量/耗时上下文，
    // 不再被回落吞掉；措辞区分「未命中」与「命中了但中途断」
    match fetch_pair(
        client,
        &mirror,
        &asset,
        false,
        &mut heartbeat,
        on_stall.clone(),
    ) {
        Ok((bytes, want)) => {
            verify_sha(&bytes, &want, &asset)?;
            eprintln!("browse update：资产取毕 {} 字节", bytes.len());
            return Ok(bytes);
        }
        Err(e) => eprintln!("browse update：镜像腿失败（{e}），回落 GitHub Releases"),
    }
    let github = format!("https://github.com/{GITHUB_REPO}/releases/download/v{version}");
    let (bytes, want) = fetch_pair(client, &github, &asset, false, &mut heartbeat, on_stall)
        .map_err(|e| {
            anyhow::anyhow!(
                "{e}；双源皆未取到 {asset}；下一步：手动升级走 GitHub Releases，\
                 或 BROWSE_RELEASE_MIRROR 换镜像源"
            )
        })?;
    verify_sha(&bytes, &want, &asset)?;
    eprintln!("browse update：资产取毕 {} 字节", bytes.len());
    Ok(bytes)
}

fn verify_sha(bytes: &[u8], want: &str, asset: &str) -> Result<()> {
    let got = format!("{:x}", Sha256::digest(bytes));
    if got != want {
        bail!(
            "sha256 不匹配（资产损坏或被篡改，硬拒不回落）：{asset} 期望 {want} 实得 {got}；\
             下一步：重试一次，仍失败反馈 browse issue new"
        );
    }
    Ok(())
}

/// 从发布包解出 browse 二进制到暂存目录。
///
/// 包形是单顶层目录（win zip，unix tar.gz）；收齐候选后要求**唯一**
/// 普通文件命中
/// （`browse`/`browse.exe`，非零字节），多命中或零命中都报错。
///
/// # Errors
///
/// 包损坏；候选零或多于一个；写盘失败。
pub fn extract_binary(archive: &[u8], staging: &Path) -> Result<PathBuf> {
    let bin_name = if cfg!(target_os = "windows") {
        "browse.exe"
    } else {
        "browse"
    };
    let dst = staging.join("new-browse");
    let mut hits: Vec<Vec<u8>> = Vec::new();
    if cfg!(target_os = "windows") {
        let cursor = std::io::Cursor::new(archive);
        let mut z =
            zip::ZipArchive::new(cursor).map_err(|e| anyhow::anyhow!("解 zip 失败：{e}"))?;
        for i in 0..z.len() {
            let mut f = z
                .by_index(i)
                .map_err(|e| anyhow::anyhow!("读 zip 条目失败：{e}"))?;
            let name = f.name().to_string();
            let is_bin = Path::new(&name)
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n == bin_name);
            if is_bin && !f.is_dir() && f.size() > 0 {
                let mut buf = Vec::new();
                std::io::Read::read_to_end(&mut f, &mut buf)?;
                hits.push(buf);
            }
        }
    } else {
        let gz = flate2::read::GzDecoder::new(archive);
        let mut tar = tar::Archive::new(gz);
        for entry in tar
            .entries()
            .map_err(|e| anyhow::anyhow!("读 tar 失败：{e}"))?
        {
            let mut e = entry.map_err(|e| anyhow::anyhow!("读 tar 条目失败：{e}"))?;
            let path = e
                .path()
                .map_err(|e| anyhow::anyhow!("读 tar 条目路径失败：{e}"))?
                .to_path_buf();
            let is_bin = path.file_name().and_then(|n| n.to_str()) == Some(bin_name);
            let is_regular = e.header().entry_type().is_file();
            if is_bin && is_regular {
                let mut buf = Vec::new();
                std::io::Read::read_to_end(&mut e, &mut buf)?;
                if !buf.is_empty() {
                    hits.push(buf);
                }
            }
        }
    }
    match hits.len() {
        1 => {
            std::fs::write(&dst, &hits[0])?;
            Ok(dst)
        }
        0 => {
            bail!("发布包内未找到 {bin_name} 二进制；下一步：重试一次，仍失败反馈 browse issue new")
        }
        n => bail!(
            "发布包内 {bin_name} 命中 {n} 件（包形异常拒绝盲取）；\
             下一步：反馈 browse issue new"
        ),
    }
}

/// exe 旁的更新锁路径（create_new 语义，占用即另一 update 在跑）。
fn lock_path(exe: &Path) -> PathBuf {
    exe.with_file_name(".browse-update.lock")
}

/// pid 后缀的备份名（并行互踩面：各 update 各自的备份）。
fn bak_path(exe: &Path) -> PathBuf {
    let pid = std::process::id();
    exe.with_file_name(format!(
        "{}.bak-{pid}",
        exe.file_name().and_then(|n| n.to_str()).unwrap_or("browse")
    ))
}

/// 回滚并复核终态：坏新件挪走、备份回位、确认 exe 在位；任何一步失败
/// 报精确自救路径。入位失败臂与自证失败臂共用。
fn rollback_and_verify(exe: &Path, bak: &Path, new_bin: &Path) -> Result<()> {
    let _ = std::fs::rename(exe, new_bin);
    match std::fs::rename(bak, exe) {
        Ok(()) if exe.exists() => Ok(()),
        Ok(()) => bail!(
            "回滚后复核 exe 缺位（旧件 {} 与新件 {} 已挪离原位）；下一步：按在位件手动复原到 {}",
            bak.display(),
            new_bin.display(),
            exe.display()
        ),
        Err(e) => bail!(
            "回滚受阻（{e}）：旧件在 {}，新件在 {}；下一步：手动复原 mv {} {} 后反馈 browse issue new",
            bak.display(),
            new_bin.display(),
            bak.display(),
            exe.display()
        ),
    }
}

/// 自替换二进制并自证回滚。
///
/// 三步舞：旧件挪 pid 备份 -> 新件入位 -> `--version` 自证（重试五次
/// 防杀软瞬时锁）-> 证毕清备份；证败回滚并**复核终态**，回滚失败报
/// 精确自救路径。
///
/// # Errors
///
/// 备份或入位失败；自证失败且回滚成功；回滚自身失败（错误带备份与
/// 新件路径及手动复原命令）。
pub fn swap_binary(exe: &Path, new_bin: &Path, expect_version: &str) -> Result<()> {
    let bak = bak_path(exe);
    std::fs::rename(exe, &bak).map_err(|e| {
        anyhow::anyhow!(
            "备份旧件失败（{} -> {}）：{e}",
            exe.display(),
            bak.display()
        )
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(new_bin)?.permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(new_bin, perm)?;
    }
    if let Err(e) = std::fs::rename(new_bin, exe) {
        rollback_and_verify(exe, &bak, new_bin).context(format!("新件入位失败：{e}"))?;
        bail!("新件入位失败（已回滚并复核在位）：{e}");
    }
    // 自证：杀软瞬时锁面重试五次（ark 同形）
    let mut probe_ok = false;
    for i in 0..5 {
        if let Ok(out) = std::process::Command::new(exe).arg("--version").output()
            && out.status.success()
            && String::from_utf8_lossy(&out.stdout).contains(expect_version)
        {
            probe_ok = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(200 * (i + 1)));
    }
    if !probe_ok {
        rollback_and_verify(exe, &bak, new_bin)
            .context(format!("新件自证失败（--version 非 {expect_version}）"))?;
        bail!(
            "新件自证失败（--version 非 {expect_version}，已回滚并复核在位）；\
             下一步：重试一次，仍失败反馈 browse issue new"
        );
    }
    std::fs::remove_file(&bak).ok();
    Ok(())
}

/// 管理方布局判据：exe 同目录 `ark-managed` 落痕，或用户面 bin 目录
/// 存在指向本 exe 的符号链接入口（ark 布局：真身进 EnvRoot，用户面
/// symlink）。返回命中物描述供错误自诊。
fn ark_managed_signal(exe: &Path) -> Option<String> {
    if let Some(d) = exe.parent()
        && d.join("ark-managed").exists()
    {
        return Some(format!("落痕 {}", d.join("ark-managed").display()));
    }
    let faces = vec![exe.parent().map(Path::to_path_buf), home_local_bin()];
    faces
        .into_iter()
        .flatten()
        .filter_map(|d| std::fs::read_dir(&d).ok())
        .flat_map(|rd| rd.flatten().map(|e| e.path()))
        .filter_map(|p| {
            let t = std::fs::read_link(&p).ok()?;
            match t.canonicalize() {
                Ok(cwd) if exe == cwd => Some(format!("链接 {} -> {}", p.display(), t.display())),
                _ => None,
            }
        })
        .next()
}

fn home_local_bin() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|h| h.join(".local").join("bin"))
}

/// `browse update` 的自更新全链入口。
///
/// GitHub latest 判新（semver 只升不降，本地领先报 localNewer 不动）；
/// 下载双通道锚校验解包；暂存落 exe 同目录（防跨文件系统 rename）；
/// 全程持锁防并行互踩；管理方布局安装拦走 ark。
/// 阻塞 http 与子进程，调用方收 `spawn_blocking`。
///
/// # Errors
///
/// 透传 [`latest_browse_version`]（判新失败）与下载校验解包自替换
/// （[`extract_binary`]/[`swap_binary`]）各步失败；ark 管理或另一
/// update 在跑时带 CTA 拒绝。
pub fn update_self() -> Result<Value> {
    let exe = std::env::current_exe().map_err(|e| anyhow::anyhow!("定位自身失败：{e}"))?;
    if let Some(signal) = ark_managed_signal(&exe) {
        bail!(
            "检测到管理方布局或用户面链接入口（{signal}）；升级建议走 ark（管理方滚 catalog pin）；\
             如确为自管安装，删该链接/落痕后再 browse update"
        );
    }
    let Some(dir) = exe.parent() else {
        bail!("exe 无父目录，无法自更新")
    };
    let _guard = acquire_lock(&exe)?;
    update_locked(&exe, dir)
}

/// 更新锁守卫：drop 时清锁件（盖 panic 面；SIGKILL 面靠 [`acquire_lock`]
/// 的 pid 陈旧收割）。
struct LockGuard {
    path: PathBuf,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// 锁内 pid 是否仍活（linux 走 /proc；他端保守判活不收割）。
fn pid_alive(pid: u32) -> bool {
    #[cfg(target_os = "linux")]
    {
        std::path::Path::new(&format!("/proc/{pid}")).exists()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        true
    }
}

/// 取更新锁：create_new 语义。AlreadyExists 先做 pid 陈旧判据（持有者
/// 已死即收割重取一次，防 SIGKILL 永久锁死）；其余 io 错误报真因（安装
/// 位不可写等），不误报「在跑」。
fn acquire_lock(exe: &Path) -> Result<LockGuard> {
    let lock = lock_path(exe);
    let take = || -> std::io::Result<std::fs::File> {
        let mut f = std::fs::File::create_new(&lock)?;
        use std::io::Write;
        let _ = writeln!(f, "{}", std::process::id());
        Ok(f)
    };
    match take() {
        Ok(_) => {
            // 陈旧收割竞态：两进程同抢时后到者回读锁 pid 让位
            let owner = std::fs::read_to_string(&lock).unwrap_or_default();
            if owner.trim() == std::process::id().to_string() {
                Ok(LockGuard { path: lock })
            } else {
                bail!(
                    "另一 browse update 正在跑（{}）；若确无 update 在跑，删 {} 后重试",
                    lock.display(),
                    lock.display()
                )
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            // 陈旧判据：锁内 pid 已死即收割重取一次
            let stale = std::fs::read_to_string(&lock)
                .ok()
                .and_then(|t| t.trim().parse::<u32>().ok())
                .is_some_and(|pid| !pid_alive(pid));
            if stale && std::fs::remove_file(&lock).is_ok() && take().is_ok() {
                return Ok(LockGuard { path: lock });
            }
            bail!(
                "另一 browse update 正在跑（{}）；若确无 update 在跑，删 {} 后重试",
                lock.display(),
                lock.display()
            );
        }
        Err(e) => bail!(
            "建更新锁失败（{}）：{e}；下一步：确认安装位可写（系统目录需提权）",
            lock.display()
        ),
    }
}

fn update_locked(exe: &Path, dir: &Path) -> Result<Value> {
    let latest = latest_browse_version()?;
    let current = env!("CARGO_PKG_VERSION");
    if latest == current {
        return Ok(json!({ "status": "upToDate", "from": current, "to": latest }));
    }
    if !version_newer(&latest, current) {
        // 本地领先（预发布或测试构建）：不降级
        return Ok(json!({
            "status": "localNewer",
            "from": current,
            "to": latest,
            "note": "本地版本更新（可能是测试构建），不降级；下一步：如确要回退走 GitHub Releases 手动装"
        }));
    }
    let client = update_http_client(std::time::Duration::from_secs(300))?;
    let bytes = fetch_asset(&client, &latest)?;
    // 暂存落 exe 同目录（跨文件系统 rename 必炸，temp 目录不可用）
    let staging = dir.join(format!(".browse-selfupd-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir(&staging)
        .map_err(|e| anyhow::anyhow!("建暂存目录失败（{}）：{e}", staging.display()))?;
    let result = (|| {
        let new_bin = extract_binary(&bytes, &staging)?;
        swap_binary(exe, &new_bin, &latest)?;
        Ok(json!({
            "status": "updated",
            "from": current,
            "to": latest,
            "exe": exe.display().to_string(),
        }))
    })();
    let _ = std::fs::remove_dir_all(&staging);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 资产名与发布流水同形（本平台三元组加包形分流）。
    #[test]
    fn asset_name_matches_release_form() {
        if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            assert_eq!(
                asset_name("0.6.1").unwrap(),
                "browse-v0.6.1-x86_64-unknown-linux-gnu.tar.gz"
            );
        } else if cfg!(target_os = "windows") {
            assert_eq!(
                asset_name("0.6.1").unwrap(),
                "browse-v0.6.1-x86_64-pc-windows-gnu.zip"
            );
        } else if cfg!(target_os = "macos") {
            assert_eq!(
                asset_name("0.6.1").unwrap(),
                "browse-v0.6.1-aarch64-apple-darwin.tar.gz"
            );
        }
    }

    /// semver 判新：数值段逐位、后缀兜底、不降级判据。
    #[test]
    fn version_newer_semantics() {
        assert!(version_newer("0.7.0", "0.6.1"));
        assert!(version_newer("0.6.2", "0.6.1"));
        assert!(version_newer("0.7.0-r2", "0.7.0-r1"));
        assert!(!version_newer("0.6.1", "0.6.1"));
        assert!(!version_newer("0.6.1", "0.7.0"), "本地领先不得判新");
        assert!(!version_newer("0.7.0", "0.7.0-r2"), "后缀旧不判新");
    }

    /// 造一个单顶层目录的 tar.gz 发布包（unix 面）。
    #[cfg(unix)]
    fn fixture_targz(dir: &Path, bin_body: &str) -> Vec<u8> {
        let src = dir.join("browse-v9.9.9");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("browse"), bin_body).unwrap();
        std::fs::write(src.join("README.md"), "stub").unwrap();
        let out = dir.join("pkg.tar.gz");
        let f = std::fs::File::create(&out).unwrap();
        let enc = flate2::write::GzEncoder::new(f, flate2::Compression::default());
        let mut b = tar::Builder::new(enc);
        b.append_dir_all("browse-v9.9.9", &src).unwrap();
        b.into_inner().unwrap().finish().unwrap();
        std::fs::read(&out).unwrap()
    }

    /// 解包选件：唯一命中取件、双命中拒绝。
    #[test]
    #[cfg(unix)]
    fn extract_binary_picks_unique_regular_file() {
        let dir = std::env::temp_dir().join("browse-selfupd-test-extract");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let pkg = fixture_targz(&dir, "#!/bin/sh\necho browse 9.9.9\n");
        let staging = dir.join("staging");
        std::fs::create_dir_all(&staging).unwrap();
        let bin = extract_binary(&pkg, &staging).unwrap();
        assert!(
            std::fs::read_to_string(&bin)
                .unwrap()
                .contains("browse 9.9.9")
        );

        // 双命中：包里塞第二个顶层目录同名件
        let src2 = dir.join("browse-v9.9.10");
        std::fs::create_dir_all(&src2).unwrap();
        std::fs::write(src2.join("browse"), "#!/bin/sh\necho decoy\n").unwrap();
        let out = dir.join("pkg2.tar.gz");
        let f = std::fs::File::create(&out).unwrap();
        let enc = flate2::write::GzEncoder::new(f, flate2::Compression::default());
        let mut b = tar::Builder::new(enc);
        b.append_dir_all("a", dir.join("browse-v9.9.9")).unwrap();
        b.append_dir_all("b", &src2).unwrap();
        b.into_inner().unwrap().finish().unwrap();
        let err = extract_binary(&std::fs::read(&out).unwrap(), &staging);
        assert!(err.is_err_and(|e| format!("{e:#}").contains("命中 2 件")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 自替换：换成功（自证过、备份清）与自证失败回滚复核两态。
    #[test]
    #[cfg(unix)]
    fn swap_binary_success_and_rollback() {
        let dir = std::env::temp_dir().join("browse-selfupd-test-swap");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("browse");
        let new_ok = dir.join("new-ok");
        let new_bad = dir.join("new-bad");
        std::fs::write(&exe, "#!/bin/sh\necho browse 0.0.1\n").unwrap();
        std::fs::write(
            &new_ok,
            "#!/bin/sh\nif [ \"$1\" = --version ]; then echo browse 9.9.9; fi\n",
        )
        .unwrap();
        std::fs::write(&new_bad, "#!/bin/sh\nexit 3\n").unwrap();
        for f in [&exe, &new_ok, &new_bad] {
            use std::os::unix::fs::PermissionsExt;
            let mut p = std::fs::metadata(f).unwrap().permissions();
            p.set_mode(0o755);
            std::fs::set_permissions(f, p).unwrap();
        }
        swap_binary(&exe, &new_ok, "9.9.9").unwrap();
        assert!(std::fs::read_to_string(&exe).unwrap().contains("9.9.9"));
        assert!(!bak_path(&exe).exists(), "成功后备份应清");

        let err = swap_binary(&exe, &new_bad, "9.9.9");
        assert!(err.is_err(), "坏新件必须拒");
        assert!(
            std::fs::read_to_string(&exe).unwrap().contains("9.9.9"),
            "回滚后旧件应在位"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 本地 mock 镜像（同 chrome_mgr 先例）：路由表 + 线程 TcpListener。
    #[cfg(unix)]
    fn mock_mirror(routes: Vec<(String, Vec<u8>)>) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { continue };
                let mut buf = [0u8; 4096];
                let Ok(n) = s.read(&mut buf) else { continue };
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = req.split_whitespace().nth(1).unwrap_or("").to_string();
                let (code, body) = match routes.iter().find(|(p, _)| *p == path) {
                    Some((_, b)) => ("200 OK", b.clone()),
                    None => ("404 Not Found", b"gone".to_vec()),
                };
                let head = format!(
                    "HTTP/1.1 {code}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(head.as_bytes());
                let _ = s.write_all(&body);
            }
        });
        format!("http://{addr}")
    }

    /// mock 镜像（记录每个连接的首行请求行，供协议断言）：路由表加日志
    /// 槽，返回 base 与首行日志（`Arc<Mutex<Vec<String>>>`）。
    #[cfg(unix)]
    fn mock_mirror_logged(
        routes: Vec<(String, Vec<u8>)>,
    ) -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = log.clone();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { continue };
                let mut buf = [0u8; 4096];
                let Ok(n) = s.read(&mut buf) else { continue };
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                sink.lock()
                    .unwrap()
                    .push(req.lines().next().unwrap_or("").to_string());
                let path = req.split_whitespace().nth(1).unwrap_or("").to_string();
                let (code, body) = match routes.iter().find(|(p, _)| *p == path) {
                    Some((_, b)) => ("200 OK", b.clone()),
                    None => ("404 Not Found", b"gone".to_vec()),
                };
                let head = format!(
                    "HTTP/1.1 {code}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(head.as_bytes());
                let _ = s.write_all(&body);
            }
        });
        (format!("http://{addr}"), log)
    }

    /// env 写面互斥锁（#54 评审 G）：HTTPS_PROXY 与 BROWSE_RELEASE_
    /// MIRROR 双写测试的 env 敏感窗互斥，防并发测试线程互相插队打到对
    /// 方 mock（SECRETS_TEST_LOCK 同形先例）。调用方全 unix 门控，同款
    /// 门控防交叉面死代码警告（d553029 模式）。
    #[cfg(unix)]
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static L: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        L.get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    /// 判新镜像优先（#54）：latest 命中即零 GitHub 依赖；缺标记回落
    /// GitHub（钉死拒连）必报其源错。GitHub 腿经 HTTPS_PROXY 指向拒连
    /// 环回隔离网络（同 fetch_asset 测试先设后建口径）。
    #[test]
    #[cfg(unix)]
    fn latest_version_mirror_first_and_fallback() {
        let _env = env_lock();
        // SAFETY: 锁内短窗覆写并保存原值，测试后还原；env_lock 保证同
        // 进程无并发 env 写读者
        let saved_proxy = std::env::var("HTTPS_PROXY").ok();
        let saved_mirror = std::env::var("BROWSE_RELEASE_MIRROR").ok();
        unsafe {
            std::env::set_var("HTTPS_PROXY", "http://127.0.0.1:9");
        }
        // 态一：镜像 latest 在 -> 判新零 GitHub
        let mirror = mock_mirror(vec![("/stable/latest".to_string(), b"9.9.9\n".to_vec())]);
        unsafe { std::env::set_var("BROWSE_RELEASE_MIRROR", &mirror) };
        assert_eq!(latest_browse_version().unwrap(), "9.9.9");

        // 态二：镜像无标记（404）-> 回落 GitHub（钉死拒连）报 GitHub 源错
        let mirror_gone = mock_mirror(vec![]);
        unsafe { std::env::set_var("BROWSE_RELEASE_MIRROR", &mirror_gone) };
        let err = latest_browse_version().unwrap_err();
        assert!(
            format!("{err:#}").contains("api.github.com"),
            "回落错应指向 GitHub 源：{err:#}"
        );

        // SAFETY: 同上，按原值还原
        unsafe {
            match saved_proxy.as_ref() {
                Some(v) => std::env::set_var("HTTPS_PROXY", v),
                None => std::env::remove_var("HTTPS_PROXY"),
            }
            match saved_mirror.as_ref() {
                Some(v) => std::env::set_var("BROWSE_RELEASE_MIRROR", v),
                None => std::env::remove_var("BROWSE_RELEASE_MIRROR"),
            }
        }
    }

    /// latest 标记解析面：单行三段数字成形；前缀、空段、垃圾页不成形。
    #[test]
    fn latest_line_parse_forms() {
        assert_eq!(parse_latest_line("0.12.3\n"), Some("0.12.3".into()));
        assert_eq!(parse_latest_line(" 0.12.3 "), Some("0.12.3".into()));
        assert_eq!(parse_latest_line("v0.12.3"), None, "v 前缀不成形");
        assert_eq!(parse_latest_line("1.2"), None, "两段不成形");
        assert_eq!(parse_latest_line("1.2.3.4"), None, "四段不成形");
        assert_eq!(parse_latest_line("1..3"), None, "空段不成形");
        assert_eq!(parse_latest_line(""), None, "空不成形");
        assert_eq!(
            parse_latest_line("<html>404 not found</html>"),
            None,
            "错误页不成形"
        );
    }

    /// 双通道三态：镜像整对优先、镜像缺回落错带双源指引、哈希不符硬拒
    /// 不回落。GitHub 腿钉死不可达（127.0.0.1:9）隔离网络；env 写面走
    /// env_lock 互斥（#54 评审 G）。
    #[test]
    #[cfg(unix)]
    fn fetch_asset_dual_channel_three_states() {
        let _env = env_lock();
        let dir = std::env::temp_dir().join("browse-selfupd-test-fetch");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let pkg = fixture_targz(&dir, "#!/bin/sh\n");
        let sha = format!("{:x}", Sha256::digest(&pkg));
        let asset = asset_name("9.9.9").unwrap();
        // 隔离网络：GitHub 回落腿硬编码基址不可钉，走 HTTPS_PROXY 指向拒连
        // 环回（reqwest 在 client build 时解析代理 env，故必须先设后建）
        // SAFETY: 锁内短窗覆写并保存原值，测试后还原；env_lock 保证同
        // 进程无并发 env 写读者
        let saved_proxy = std::env::var("HTTPS_PROXY").ok();
        let saved_mirror = std::env::var("BROWSE_RELEASE_MIRROR").ok();
        unsafe {
            std::env::set_var("HTTPS_PROXY", "http://127.0.0.1:9");
        }
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(2))
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap();

        // 态一：镜像整对在 -> 取到
        let mirror = mock_mirror(vec![
            (
                format!("/stable/{asset}.sha256"),
                format!("{sha}  x\n").into_bytes(),
            ),
            (format!("/stable/{asset}"), pkg.clone()),
        ]);
        unsafe { std::env::set_var("BROWSE_RELEASE_MIRROR", &mirror) };
        let got = fetch_asset(&client, "9.9.9").unwrap();
        assert_eq!(got, pkg, "镜像整对在应取到");

        // 态二：镜像 404 -> 回落 GitHub（钉死拒连）必报双源错
        let mirror_gone = mock_mirror(vec![]);
        unsafe { std::env::set_var("BROWSE_RELEASE_MIRROR", &mirror_gone) };
        let err = fetch_asset(&client, "9.9.9").unwrap_err();
        assert!(
            format!("{err:#}").contains("双源皆未取到"),
            "回落错应带双源指引：{err:#}"
        );

        // 态三：镜像资产被篡改（边车锚对不上）-> 硬拒不回落
        let mut tampered = pkg.clone();
        tampered.extend_from_slice(b"TAMPER");
        let mirror_bad = mock_mirror(vec![
            (
                format!("/stable/{asset}.sha256"),
                format!("{sha}  x\n").into_bytes(),
            ),
            (format!("/stable/{asset}"), tampered),
        ]);
        unsafe { std::env::set_var("BROWSE_RELEASE_MIRROR", &mirror_bad) };
        let err = fetch_asset(&client, "9.9.9").unwrap_err();
        assert!(
            format!("{err:#}").contains("sha256 不匹配"),
            "篡改必须硬拒：{err:#}"
        );

        // SAFETY: 同上，按原值还原
        unsafe {
            match saved_proxy.as_ref() {
                Some(v) => std::env::set_var("HTTPS_PROXY", v),
                None => std::env::remove_var("HTTPS_PROXY"),
            }
            match saved_mirror.as_ref() {
                Some(v) => std::env::set_var("BROWSE_RELEASE_MIRROR", v),
                None => std::env::remove_var("BROWSE_RELEASE_MIRROR"),
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 总台令 2026-09-23（随 #58 链）：镜像下载腿恒 HTTP/1.1——对 mock
    /// 镜像取 >2MB（>1,720,320B 悬崖）资产，观测镜像域请求首行全为
    /// `HTTP/1.1` 且全量送达过 sha 锚。这是「不落 h2」的回归锁：任何 h2c
    /// 客户端首行是 `PRI * HTTP/2.0`，会当场破坏该断言（reqwest 日后被
    /// feature 统一化打开 http2 而漂回协商时本测即红）。
    #[test]
    #[cfg(unix)]
    fn mirror_download_leg_forces_http1_above_cliff() {
        let _env = env_lock();
        const BIG: usize = 2_500_000; // 过 2MB 线与 1,720,320B 悬崖
        let asset = asset_name("9.9.9").unwrap();
        let pkg = vec![b'A'; BIG];
        let sha = format!("{:x}", Sha256::digest(&pkg));
        let (mirror, log) = mock_mirror_logged(vec![
            (
                format!("/stable/{asset}.sha256"),
                format!("{sha}  x\n").into_bytes(),
            ),
            (format!("/stable/{asset}"), pkg.clone()),
        ]);
        let saved_mirror = std::env::var("BROWSE_RELEASE_MIRROR").ok();
        // SAFETY: env_lock 窗内独占改 env，块尾按原值还原
        unsafe { std::env::set_var("BROWSE_RELEASE_MIRROR", &mirror) };
        let client = update_http_client(std::time::Duration::from_secs(30)).unwrap();
        let got = fetch_asset(&client, "9.9.9").expect("镜像腿应取到 >2MB 资产");
        assert_eq!(got.len(), BIG, ">2MB 资产应全量送达（悬崖腿修复前断流）");
        let lines = log.lock().unwrap().clone();
        assert!(
            lines
                .iter()
                .any(|l| *l == format!("GET /stable/{asset} HTTP/1.1")),
            "镜像资产请求应为 HTTP/1.1：{lines:?}"
        );
        assert!(
            !lines.is_empty() && lines.iter().all(|l| l.ends_with("HTTP/1.1")),
            "镜像域请求不得走 h2（首行非 h1）：{lines:?}"
        );
        // SAFETY: 同一锁窗内按原值还原
        unsafe {
            match saved_mirror.as_ref() {
                Some(v) => std::env::set_var("BROWSE_RELEASE_MIRROR", v),
                None => std::env::remove_var("BROWSE_RELEASE_MIRROR"),
            }
        }
    }

    /// 慢源 mock（#58）：body 按 chunk 分片写、片间 sleep，模拟限速源
    /// （服务端节流等价限速代理）；截断形由 truncate_to 控制声明长度
    /// 后只写部分即关（模拟断流）；stall 形在累计写出 N 字节后长睡
    /// （模拟连接活但零字节的停顿，G1）。
    fn mock_mirror_slow(
        route_path: &str,
        body: Vec<u8>,
        chunk: usize,
        delay_ms: u64,
        truncate_to: Option<usize>,
        stall: Option<(usize, u64)>,
    ) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let path = route_path.to_string();
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { continue };
                let mut buf = [0u8; 4096];
                let Ok(n) = s.read(&mut buf) else { continue };
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                if req.split_whitespace().nth(1).unwrap_or("") != path {
                    let _ = s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 5\r\n\r\ngone");
                    continue;
                }
                // 声明全长（截断形也全长，制造 Content-Length 与实发不符）
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(head.as_bytes());
                let send = &body[..truncate_to.unwrap_or(body.len()).min(body.len())];
                let mut sent = 0usize;
                for piece in send.chunks(chunk.max(1)) {
                    if let Some((at, ms)) = stall
                        && sent <= at
                        && sent + piece.len() > at
                    {
                        std::thread::sleep(std::time::Duration::from_millis(ms));
                    }
                    let _ = s.write_all(piece);
                    let _ = s.flush();
                    sent += piece.len();
                    if delay_ms > 0 {
                        std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                    }
                }
                let _ = s.shutdown(std::net::Shutdown::Write);
            }
        });
        format!("http://{addr}")
    }

    /// #58 心跳面（验收 1/4）：限速源（服务端节流）下进度回调按节流闸
    /// 周期出现、字节单调、不全量（完成态不回调）、终值完整送达。
    #[test]
    fn download_progress_heartbeat_on_slow_source() {
        const TOTAL: usize = 300 * 1024;
        let base = mock_mirror_slow("/a", vec![b'B'; TOTAL], 8 * 1024, 5, None, None);
        let url = format!("{base}/a");
        let client = update_http_client(std::time::Duration::from_secs(30)).unwrap();
        let resp = client.get(&url).send().unwrap().error_for_status().unwrap();
        let mut events: Vec<(u64, Option<u64>)> = Vec::new();
        let bytes = read_body_with_progress(
            resp,
            "a",
            &mut ProgressHooks {
                min_bytes: 8 * 1024,
                min_interval: std::time::Duration::from_millis(30),
                stall_interval: std::time::Duration::from_secs(3600),
                tick: std::time::Duration::from_secs(1),
                on_progress: &mut |got, total| events.push((got, total)),
                on_stall: std::sync::Arc::new(std::sync::Mutex::new(|_, _| {})),
            },
        )
        .expect("慢源全量送达");
        assert_eq!(bytes.len(), TOTAL, "全量完整");
        assert!(
            events.len() >= 3,
            "限速源应多拍心跳（300KB/8KB 片加 30ms 闸）: {events:?}"
        );
        assert!(
            events
                .iter()
                .all(|(g, t)| *t == Some(TOTAL as u64) && *g < TOTAL as u64),
            "心跳带总量且未到完成态: {events:?}"
        );
        assert!(
            events.windows(2).all(|w| w[0].0 <= w[1].0),
            "字节序单调: {events:?}"
        );
    }

    /// #58 超时/断流可预期（验收 2）：声明长度后中途断流，错误信息带
    /// 已收/总量/耗时上下文。
    #[test]
    fn download_error_carries_progress_context() {
        const TOTAL: usize = 128 * 1024;
        let base = mock_mirror_slow("/a", vec![b'C'; TOTAL], 8 * 1024, 0, Some(TOTAL / 2), None);
        let url = format!("{base}/a");
        let client = update_http_client(std::time::Duration::from_secs(30)).unwrap();
        let resp = client.get(&url).send().unwrap().error_for_status().unwrap();
        let err = read_body_with_progress(
            resp,
            "the-asset",
            &mut ProgressHooks {
                min_bytes: 8 * 1024,
                min_interval: std::time::Duration::from_millis(30),
                stall_interval: std::time::Duration::from_secs(3600),
                tick: std::time::Duration::from_secs(1),
                on_progress: &mut |_, _| {},
                on_stall: std::sync::Arc::new(std::sync::Mutex::new(|_, _| {})),
            },
        )
        .expect_err("半途断流应错");
        let msg = format!("{err:#}");
        assert!(msg.contains("the-asset"), "带资产名: {msg}");
        assert!(
            msg.contains(&format!("{}/{}", TOTAL / 2, TOTAL)),
            "带已收/总量上下文: {msg}"
        );
        assert!(msg.contains("耗时"), "带耗时: {msg}");
    }

    /// G1 stall 看门狗（用户令随批）：连接活但零字节的停顿经独立线程
    /// 周期告警（已收字节与停顿秒数），停顿结束后续传不受扰、终值全量。
    #[test]
    fn download_stall_watchdog_warns_on_zero_byte_pause() {
        const TOTAL: usize = 128 * 1024;
        // 64KB 后长睡 450ms（stall），看门狗 150ms 闸 50ms 节拍应至少两拍
        let base = mock_mirror_slow(
            "/a",
            vec![b'D'; TOTAL],
            16 * 1024,
            0,
            None,
            Some((64 * 1024, 450)),
        );
        let url = format!("{base}/a");
        let client = update_http_client(std::time::Duration::from_secs(30)).unwrap();
        let resp = client.get(&url).send().unwrap().error_for_status().unwrap();
        let stalls: std::sync::Arc<std::sync::Mutex<Vec<(u64, u64)>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = stalls.clone();
        let bytes = read_body_with_progress(
            resp,
            "a",
            &mut ProgressHooks {
                min_bytes: u64::MAX,
                min_interval: std::time::Duration::from_millis(20),
                stall_interval: std::time::Duration::from_millis(150),
                tick: std::time::Duration::from_millis(50),
                on_progress: &mut |_, _| {},
                on_stall: std::sync::Arc::new(std::sync::Mutex::new(move |got, stalled| {
                    sink.lock().unwrap().push((got, stalled))
                })),
            },
        )
        .expect("停顿后续传应全量送达");
        assert_eq!(bytes.len(), TOTAL, "stall 不影响终值");
        let events = stalls.lock().unwrap().clone();
        assert!(
            !events.is_empty(),
            "零字节停顿应触发看门狗告警（450ms 停顿对 150ms 闸）"
        );
        assert!(
            events.iter().all(|(got, _s)| *got == 64 * 1024),
            // 秒数是 as_secs 截断形（测试停顿亚秒得 0），字节锚已足证
            "告警带停顿点的已收字节: {events:?}"
        );
    }

    /// 文案锁（评审 G 尾项）：心跳与 stall 行的措辞由纯函数锁形。
    #[test]
    fn progress_line_wording_locked() {
        assert_eq!(
            heartbeat_line("browse update", 1024, Some(4096)),
            "browse update：下载中 1024/4096（25%）"
        );
        assert_eq!(
            heartbeat_line("browse update", 0, Some(4096)),
            "browse update：下载中 0/4096（0%）"
        );
        assert_eq!(
            heartbeat_line("browse update", 7, None),
            "browse update：下载中 7 字节（总量未知）"
        );
        assert_eq!(
            stall_line("browse update", 2048, 20),
            "browse update：仍在等数据（已收 2048 字节，20 秒无进展）"
        );
    }
}
