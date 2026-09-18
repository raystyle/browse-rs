//! browse 自更新（用户令 2026-09-18；对齐 build-release 公共契约第六节
//! 双通道）：GitHub Releases latest 判新（semver 只升不降）-> 下载本平台
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
pub const GITHUB_REPO: &str = "raystyle/browse-rs";

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

/// 发现最新版本号。
///
/// GitHub Releases latest API（tag 去 `v` 前缀）；镜像 stable 段是下载
/// 通道非判新源（段内资产名带版本，无法反查最新号）。
///
/// 阻塞 http，调用方收 `spawn_blocking`。
///
/// # Errors
///
/// API 不可达、限流（403/429 带 token 指引）或未回 tag。
pub fn latest_browse_version() -> Result<String> {
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| anyhow::anyhow!("构建 http client 失败：{e}"))?;
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

/// 从一个源取「边车 + 资产」对（边车先行省流量）；任何一步失败即该源
/// 整对作废（防资产与边车混源）。
fn fetch_pair(
    client: &reqwest::blocking::Client,
    base: &str,
    asset: &str,
    api: bool,
) -> Result<(Vec<u8>, String)> {
    let sidecar_url = format!("{base}/{asset}.sha256");
    let sidecar = http_get(client, &sidecar_url, api)?
        .text()
        .map_err(|e| anyhow::anyhow!("读边车 body 失败：{e}"))?;
    let want = sidecar.split_whitespace().next().unwrap_or("");
    if want.len() != 64 || !want.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("边车 {sidecar_url} 内容非法（要 64 位十六进制）");
    }
    let bytes = http_get(client, &format!("{base}/{asset}"), api)?
        .bytes()
        .map_err(|e| anyhow::anyhow!("读资产 body 失败：{e}"))?
        .to_vec();
    Ok((bytes, want.to_string()))
}

/// 双通道下载加锚校验：镜像 stable 段整对优先（资产或边车任一 404/失败
/// 即整对回落 GitHub）；哈希不符是安全问题，**硬拒不回落**。
fn fetch_asset(client: &reqwest::blocking::Client, version: &str) -> Result<Vec<u8>> {
    let asset = asset_name(version)?;
    let mirror = format!("{}/stable", mirror_base());
    eprintln!("browse update：镜像 stable 段取 {asset}");
    if let Ok((bytes, want)) = fetch_pair(client, &mirror, &asset, false) {
        verify_sha(&bytes, &want, &asset)?;
        return Ok(bytes);
    }
    let github = format!("https://github.com/{GITHUB_REPO}/releases/download/v{version}");
    eprintln!("browse update：镜像未命中，回落 GitHub Releases");
    let (bytes, want) = fetch_pair(client, &github, &asset, false).map_err(|e| {
        anyhow::anyhow!(
            "{e}；双源皆未取到 {asset}；下一步：手动升级走 GitHub Releases，\
             或 BROWSE_RELEASE_MIRROR 换镜像源"
        )
    })?;
    verify_sha(&bytes, &want, &asset)?;
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
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| anyhow::anyhow!("构建 http client 失败：{e}"))?;
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

    /// 双通道三态（仓内锁）：镜像整对优先、镜像缺回落错带双源指引、
    /// 哈希不符硬拒不回落。GitHub 腿钉死不可达（127.0.0.1:9）隔离网络。
    #[test]
    #[cfg(unix)]
    fn fetch_asset_dual_channel_three_states() {
        let dir = std::env::temp_dir().join("browse-selfupd-test-fetch");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let pkg = fixture_targz(&dir, "#!/bin/sh\n");
        let sha = format!("{:x}", Sha256::digest(&pkg));
        let asset = asset_name("9.9.9").unwrap();
        // 隔离网络：GitHub 回落腿硬编码基址不可钉，走 HTTPS_PROXY 指向拒连
        // 环回（reqwest 在 client build 时解析代理 env，故必须先设后建）
        // SAFETY: 测试进程短窗覆写并保存原值，测试后还原；并发测试无代理读者
        let saved_proxy = std::env::var("HTTPS_PROXY").ok();
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
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
