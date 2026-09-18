//! 内嵌 Chromium 版本管理器（ADR-0007）：各版本 clean-chrome 在本仓应用
//! 数据目录下版本化管理（安装、pin、升级、体检）。
//!
//! 布局：`<state>/chromium/<version>/`（每版本一目录，旧版保留可回退），
//! 登记 `<state>/chromium/manifest.json`（已装版本 + pin 指向）。
//! 引擎 user-data 不在版本目录（engine-profile 跨版本持久，升级零迁移）。
//!
//! 安装源两形（ADR-0007）：本地目录导入（SxS 部署形态，`chromium-<ver>/`
//! 整目录复制）；R2 镜像版本段下载（chrome.ohmygh.com，`<ver>/<asset>` 加
//! 同名 `.sha256` 边车锚，总台热验 2026-09-17 回执；资产名是定标形
//! `chromium-<version>-<三元组>.zip`，`BROWSE_CHROME_ASSET` 可覆写）。

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

/// 一条已安装版本的登记项。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChromeInstall {
    /// 版本号（即 `<root>/<version>/` 目录名）。
    pub version: String,
    /// 安装来源描述（本地导入为 `dir:<path>`）。
    pub source: String,
    /// 文件数（体检基线）。
    pub files: u64,
    /// 字节数（体检基线）。
    pub bytes: u64,
    /// 安装时刻（Unix 毫秒）。
    pub installed_at: u64,
}

/// 版本登记册：已装版本 + 当前 pin。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChromeManifest {
    /// 已装版本清单（按安装序）。
    pub installed: Vec<ChromeInstall>,
    /// 当前 pin 的版本（发现序的托管位生效点）。
    pub pinned: Option<String>,
}

/// 校验版本字符串（同时是目录名）：限 `[A-Za-z0-9._-]`，防路径穿越。
fn valid_version(v: &str) -> Result<()> {
    let ok = !v.is_empty()
        && v.len() <= 64
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if ok {
        Ok(())
    } else {
        bail!(
            "版本号 {v:?} 非法（只允许字母数字与 . _ -，至多 64 字符）；\
             下一步：用 chromeInstall 传入目录名自带的版本（如 chromium-152.0.7977.84 的 152.0.7977.84）"
        )
    }
}

/// 从部署目录名提取版本（`chromium-152.0.7977.84` 出 `152.0.7977.84`；
/// 无前缀则整名当版本）。
pub fn version_from_dir_name(dir: &Path) -> String {
    let name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string();
    name.strip_prefix("chromium-").unwrap_or(&name).to_string()
}

/// 托管根目录（本实例状态目录下的 `chromium/`）。
pub fn chromium_root() -> PathBuf {
    crate::paths::state_dir().join("chromium")
}

/// 某版本的落位目录（不校验存在）。
pub fn version_dir(root: &Path, version: &str) -> PathBuf {
    root.join(version)
}

/// manifest 落盘路径。
pub fn manifest_path(root: &Path) -> PathBuf {
    root.join("manifest.json")
}

/// 读登记册；根目录不存在或 manifest 缺失视为空册。
pub fn read_manifest(root: &Path) -> ChromeManifest {
    std::fs::read_to_string(manifest_path(root))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// 写登记册（先建根目录）。
///
/// # Errors
///
/// 根目录建不出来或序列化写盘失败。
pub fn write_manifest(root: &Path, m: &ChromeManifest) -> Result<()> {
    std::fs::create_dir_all(root)?;
    std::fs::write(
        manifest_path(root),
        serde_json::to_vec_pretty(m).unwrap_or_default(),
    )?;
    Ok(())
}

/// 校验某目录是可用的 Chromium 部署（chrome 二进制在位）。
///
/// # Errors
///
/// 目录不存在或没有 chrome 二进制（错误带 chromeInstall CTA）。
pub fn check_deployed(dir: &Path) -> Result<()> {
    match cdp::spawn::chrome_binary_in_dir(dir) {
        Some(_) => Ok(()),
        None => bail!(
            "{} 不是可用的 Chromium 部署（{} 内无 chrome 二进制；mac 是 Chromium.app 束）；\
             下一步：chromeInstall({{fromDir: \"<部署目录>\"}}) 指向含 chrome 二进制的目录",
            dir.display(),
            dir.display()
        ),
    }
}

/// 递归复制目录树，返回 `(文件数, 字节数)`。
fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<(u64, u64)> {
    std::fs::create_dir_all(dst)?;
    let mut files = 0u64;
    let mut bytes = 0u64;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        let to = dst.join(entry.file_name());
        if ft.is_dir() {
            let (f, b) = copy_tree(&entry.path(), &to)?;
            files += f;
            bytes += b;
        } else if ft.is_file() {
            std::fs::copy(entry.path(), &to)?;
            files += 1;
            bytes += entry.metadata().map(|m| m.len()).unwrap_or(0);
        }
        // 符号链接不复制（Chromium 部署形态不含）
    }
    Ok((files, bytes))
}

/// 从本地部署目录导入安装一个版本（整目录复制到 `<root>/<version>/`），
/// 登记进 manifest 并 pin 为当前版本。
///
/// # Errors
///
/// 版本号非法、源目录不是可用部署、目标已存在（不覆盖，先显式删）、
/// 复制或写盘失败。
pub fn install_from_dir(root: &Path, version: &str, from_dir: &Path) -> Result<Value> {
    valid_version(version)?;
    check_deployed(from_dir)?;
    let dst = version_dir(root, version);
    if dst.exists() {
        bail!(
            "版本 {version} 已安装（{}）；下一步：换版本号，或手动删该目录后重装（升级走新版本号 install）",
            dst.display()
        );
    }
    // mac `.app` 束导入保留束形（版本目录内存 Chromium.app）；其他平台整树平铺
    let target = if from_dir.extension().is_some_and(|e| e == "app") {
        dst.join(from_dir.file_name().unwrap_or_default())
    } else {
        dst.clone()
    };
    let (files, bytes) = copy_tree(from_dir, &target).map_err(|e| {
        anyhow::anyhow!(
            "复制部署失败（{} -> {}）：{e}",
            from_dir.display(),
            target.display()
        )
    })?;
    check_deployed(&dst)?;
    let mut m = read_manifest(root);
    let rec = ChromeInstall {
        version: version.to_string(),
        source: format!("dir:{}", from_dir.display()),
        files,
        bytes,
        installed_at: now_ms(),
    };
    m.installed.retain(|i| i.version != version);
    m.installed.push(rec.clone());
    m.pinned = Some(version.to_string());
    write_manifest(root, &m)?;
    Ok(install_json(&rec))
}

/// R2 镜像默认基址（chrome.ohmygh.com 版本段路由，ADR-0007 决策二；
/// `BROWSE_CHROME_MIRROR` 覆写走测试或自建镜像）。
pub const DEFAULT_MIRROR: &str = "https://chrome.ohmygh.com";

/// 资产名（定标形，总台裁二）：`chromium-<version>-<三元组>.zip`；三元组是
/// clean-chrome 构建面（Windows msvc、Linux gnu、mac arm64），与本仓自身
/// 编译面无关；`BROWSE_CHROME_ASSET` 全名覆写不变。
pub fn asset_name(version: &str) -> String {
    format!("chromium-{version}-{}.zip", chromium_triple())
}

/// clean-chrome 资产的构建三元组（按运行平台选包）。
fn chromium_triple() -> &'static str {
    if cfg!(target_os = "windows") {
        "x86_64-pc-windows-msvc"
    } else if cfg!(target_os = "macos") {
        "aarch64-apple-darwin"
    } else {
        "x86_64-unknown-linux-gnu"
    }
}

/// 镜像基址与资产名解析（`BROWSE_CHROME_MIRROR` / `BROWSE_CHROME_ASSET`
/// 环境覆写，空值忽略；测试与程序化调用走 [`install_from_mirror_with`]）。
fn mirror_and_asset(version: &str) -> (String, String) {
    let mirror = std::env::var("BROWSE_CHROME_MIRROR")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_MIRROR.to_string())
        .trim_end_matches('/')
        .to_string();
    let asset = std::env::var("BROWSE_CHROME_ASSET")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| asset_name(version));
    (mirror, asset)
}

/// 从 R2 镜像下载安装一个版本（环境覆写形态；见 [`install_from_mirror_with`]）。
///
/// # Errors
///
/// 同 [`install_from_mirror_with`]。
pub fn install_from_mirror(root: &Path, version: &str) -> Result<Value> {
    let (mirror, asset) = mirror_and_asset(version);
    install_from_mirror_with(root, version, &mirror, &asset)
}

/// 带显式镜像基址与资产名的下载安装（env 包装的内核，测试与程序化面）：
/// `<mirror>/<version>/<asset>` 下载加同名 `.sha256` 边车锚校验，
/// zip 解包后原子落位 `<root>/<version>/`，manifest 登记并自动 pin。
///
/// # Errors
///
/// 版本号非法或目标已存在（同本地导入）；边车或资产 404（错误带端点与
/// 覆写指引）；sha256 不匹配（错包即弃，不留残目录）；边车内容非法；
/// 解包后无 chrome 二进制；下载或写盘失败。
pub fn install_from_mirror_with(
    root: &Path,
    version: &str,
    mirror: &str,
    asset: &str,
) -> Result<Value> {
    valid_version(version)?;
    let dst = version_dir(root, version);
    if dst.exists() {
        bail!(
            "版本 {version} 已安装（{}）；下一步：换版本号，或手动删该目录后重装",
            dst.display()
        );
    }
    let mirror = mirror.trim_end_matches('/');
    let base = format!("{mirror}/{version}/{asset}");
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| anyhow::anyhow!("构建 http client 失败：{e}"))?;

    // 边车锚先行：锚不在即端点无此资产，不必拉大包
    let sidecar = client
        .get(format!("{base}.sha256"))
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|e| mirror_cta(&base, e))?
        .text()
        .map_err(|e| anyhow::anyhow!("读 {base}.sha256 失败：{e}"))?;
    // 边车是 sha256sum -c 兼容格式（hex 双空格 文件名），取首 token 为锚
    let want = sidecar.split_whitespace().next().unwrap_or("");
    if want.len() != 64 || !want.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!(
            "{base}.sha256 边车内容非法（要 64 位十六进制，得 {:?}）；\
             下一步：检查镜像资产是否完整",
            want.chars().take(20).collect::<String>()
        );
    }

    // 下载到 staging（同文件系统，rename 才原子）
    let staging = root.join(format!(".staging-{version}"));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;
    let job = MirrorJob {
        client: &client,
        base: &base,
        want,
        staging: &staging,
        asset,
        version,
        mirror,
    };
    let result = install_from_mirror_inner(root, job);
    let _ = std::fs::remove_dir_all(&staging);
    result
}

/// 下载腿内核入参束（调用方拼好 URL 与 staging；内核只管下载到落位全链）。
struct MirrorJob<'a> {
    /// http 客户端（带连接超时）。
    client: &'a reqwest::blocking::Client,
    /// 资产全 URL（`<mirror>/<version>/<asset>`）。
    base: &'a str,
    /// 边车锚期望值（64 位十六进制）。
    want: &'a str,
    /// staging 目录（同文件系统，原子 rename 的前提）。
    staging: &'a Path,
    /// 资产文件名（staging 内落盘名）。
    asset: &'a str,
    /// 版本号。
    version: &'a str,
    /// 镜像基址（错误提示用）。
    mirror: &'a str,
}

/// 下载腿主体（staging 已就位；错误统一向上带 CTA）。
fn install_from_mirror_inner(root: &Path, job: MirrorJob) -> Result<Value> {
    let MirrorJob {
        client,
        base,
        want,
        staging,
        asset,
        version,
        mirror,
    } = job;
    let zip_path = staging.join(asset);
    let dst = version_dir(root, version);
    let mut resp = client
        .get(base)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|e| mirror_cta(base, e))?;
    let mut file = std::fs::File::create(&zip_path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = resp
            .read(&mut buf)
            .map_err(|e| anyhow::anyhow!("下载 {base} 中断：{e}"))?;
        if n == 0 {
            break;
        }
        std::io::Write::write_all(&mut file, &buf[..n])?;
        hasher.update(&buf[..n]);
    }
    drop(file);
    let got = format!("{:02x}", hasher.finalize());
    if got != want {
        bail!(
            "{base} 的 sha256 不匹配（边车锚 {want}，实得 {got}）；\
             下一步：重试；仍不匹配即镜像资产损坏，勿装",
        );
    }

    // 解包（zip 内单顶层目录则下钻一层，兼容 chromium-<ver>/ 打包形）
    let extract_dir = staging.join("x");
    std::fs::create_dir_all(&extract_dir)?;
    let archive = std::fs::File::open(zip_path)?;
    let mut zip =
        zip::ZipArchive::new(archive).map_err(|e| anyhow::anyhow!("{base} 不是可用 zip：{e}"))?;
    zip.extract(&extract_dir)
        .map_err(|e| anyhow::anyhow!("解包 {base} 失败：{e}"))?;
    let effective = single_top_dir(&extract_dir).unwrap_or_else(|| extract_dir.clone());
    check_deployed(&effective).map_err(|e| {
        anyhow::anyhow!(
            "{e}；镜像包内容形不对（定标形 {}/{version}/ 内是部署目录）",
            mirror
        )
    })?;
    let (files, bytes) = count_tree(&effective);
    std::fs::rename(&effective, &dst).map_err(|e| {
        anyhow::anyhow!("落位 {} 失败：{e}（同盘 rename，不应跨盘）", dst.display())
    })?;

    let mut m = read_manifest(root);
    let rec = ChromeInstall {
        version: version.to_string(),
        source: format!("mirror:{mirror}"),
        files,
        bytes,
        installed_at: now_ms(),
    };
    m.installed.retain(|i| i.version != version);
    m.installed.push(rec.clone());
    m.pinned = Some(version.to_string());
    write_manifest(root, &m)?;
    Ok(install_json(&rec))
}

/// 镜像腿错误统一加 CTA：端点、资产名覆写、首版资产窗口。
fn mirror_cta(base: &str, e: reqwest::Error) -> anyhow::Error {
    anyhow::anyhow!(
        "镜像取 {base} 失败：{e}；下一步：核对版本号；资产名非定标形时设 \
         BROWSE_CHROME_ASSET 全名覆写；clean-chrome 首版资产未落桶前 404 属预期"
    )
}

/// 若目录恰含一个子目录且无散文件，返回该子目录（zip 单顶层目录形）。
fn single_top_dir(dir: &Path) -> Option<PathBuf> {
    let mut entries = std::fs::read_dir(dir).ok()?.collect::<Vec<_>>();
    entries.retain(|e| e.is_ok());
    if entries.len() != 1 {
        return None;
    }
    let p = entries[0].as_ref().ok()?.path();
    p.is_dir().then_some(p)
}

/// 数目录树 `(文件数, 字节数)`（体检基线用）。
fn count_tree(dir: &Path) -> (u64, u64) {
    let mut files = 0u64;
    let mut bytes = 0u64;
    if let Ok(rd) = std::fs::read_dir(dir) {
        for entry in rd.flatten() {
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_dir() {
                let (f, b) = count_tree(&entry.path());
                files += f;
                bytes += b;
            } else if ft.is_file() {
                files += 1;
                bytes += entry.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    (files, bytes)
}

/// pin 切换到已装版本（引擎发现序的托管位生效点）。
///
/// # Errors
///
/// 版本未安装（错误带 chromeList/chromeInstall CTA）。
pub fn use_version(root: &Path, version: &str) -> Result<Value> {
    let mut m = read_manifest(root);
    if !m.installed.iter().any(|i| i.version == version) {
        let have: Vec<&str> = m.installed.iter().map(|i| i.version.as_str()).collect();
        bail!(
            "版本 {version} 未安装（已装：[{}]）；\
             下一步：chromeInstall({{fromDir: \"chromium-{version}\"}}) 先装，或 chromeList() 看已装",
            have.join(", ")
        );
    }
    m.pinned = Some(version.to_string());
    write_manifest(root, &m)?;
    Ok(json!({ "pinned": version }))
}

/// 发现镜像最新版本：优先 `BROWSE_CHROME_LATEST` 环境钉（离线与测试面），
/// 缺省读 `<mirror>/latest.txt` 单行版本号（155 前的过渡发现口径，端点候
/// omc 落桶；版本发现正式定标在 REQ-003 余量）。
///
/// 阻塞 http，调用方须收在 `spawn_blocking` 里（async 上下文 drop 该
/// client 会 panic，与镜像安装腿同规）。
///
/// # Errors
///
/// `latest.txt` 404 或不可达（错误带过渡指引：显式装或环境钉）；返回
/// 内容不是合法版本号（限字母数字与 `. _ -`，同版本目录名口径）。
pub fn latest_version() -> Result<String> {
    if let Some(v) = std::env::var("BROWSE_CHROME_LATEST")
        .ok()
        .filter(|s| !s.is_empty())
    {
        let v = v.trim().to_string();
        valid_version(&v)?;
        return Ok(v);
    }
    let (mirror, _) = mirror_and_asset("");
    let url = format!("{mirror}/latest.txt");
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| anyhow::anyhow!("构建 http client 失败：{e}"))?;
    let body = client
        .get(&url)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|e| {
            anyhow::anyhow!(
                "读 {url} 失败（{e}）；镜像暂无 latest.txt 发现端点（155 前过渡口径）；\
                 下一步：browse chrome install <版本> 显式装，或 BROWSE_CHROME_LATEST=<版本> 钉住发现源"
            )
        })?
        .text()
        .map_err(|e| anyhow::anyhow!("读 {url} body 失败：{e}"))?;
    let v = body.trim().lines().last().unwrap_or("").trim().to_string();
    valid_version(&v)?;
    Ok(v)
}

/// `browse chrome update`：发现最新版（[`latest_version`]），未装则镜像
/// 安装，再把托管 pin 切过去（默认使用最新版）；已是该版时幂等。
/// 阻塞 http，调用方收 `spawn_blocking`。
///
/// # Errors
///
/// 透传 [`latest_version`]（发现失败）与 [`install_from_mirror_with`]
/// （下载安装失败）；pin 切换见 [`use_version`]。
pub fn update(root: &Path) -> Result<Value> {
    let latest = latest_version()?;
    update_with(root, &latest)
}

/// 带显式版本的 update 内核（测试与程序化面，不触发现端点）：未装则装
/// （镜像腿），pin 切到该版。
///
/// # Errors
///
/// 同 [`update`]（除发现失败）。
pub fn update_with(root: &Path, latest: &str) -> Result<Value> {
    valid_version(latest)?;
    let previous = read_manifest(root).pinned;
    let installed_now = !version_dir(root, latest).exists();
    if installed_now {
        install_from_mirror(root, latest)?;
    }
    use_version(root, latest)?;
    Ok(json!({
        "version": latest,
        "installedNow": installed_now,
        "previousPin": previous,
        "pinned": latest,
    }))
}

/// 删除一个已装版本：目录与 manifest 登记一起清，回执带释放的文件数与
/// 字节数（取自登记基线）。**pin 指向的版本拒删**（保「pin 永远指向在位
/// 版本」不变量，doctor 的 pinOk 不破）；先 [`use_version`] 切走或先装
/// 新版再删。
///
/// # Errors
///
/// 版本未安装（错误带已装清单 CTA）；版本是当前 pin（错误带先切 CTA）；
/// 目录删除失败。
pub fn remove_version(root: &Path, version: &str) -> Result<Value> {
    valid_version(version)?;
    let mut m = read_manifest(root);
    let Some(rec) = m.installed.iter().find(|i| i.version == version) else {
        let have: Vec<&str> = m.installed.iter().map(|i| i.version.as_str()).collect();
        bail!(
            "版本 {version} 未安装（已装：[{}]）；下一步：browse chrome list 核对版本号",
            have.join(", ")
        );
    };
    if m.pinned.as_deref() == Some(version) {
        bail!(
            "版本 {version} 是当前 pin（引擎托管位正用它）；下一步：browse chrome use <其他已装版> \
             先切走再删；若只有这一个版本，先 browse chrome install <新版> 装新再切再删"
        );
    }
    let rec = rec.clone();
    let dir = version_dir(root, version);
    std::fs::remove_dir_all(&dir).map_err(|e| anyhow::anyhow!("删 {} 失败：{e}", dir.display()))?;
    m.installed.retain(|i| i.version != version);
    write_manifest(root, &m)?;
    Ok(json!({
        "removed": version,
        "files": rec.files,
        "bytes": rec.bytes,
        "pinned": m.pinned,
    }))
}

/// 列已装版本与 pin（给 chromeList 面与 CLI）。
pub fn list_json(root: &Path) -> Value {
    let m = read_manifest(root);
    json!({
        "root": root.display().to_string(),
        "pinned": m.pinned,
        "installed": m.installed.iter().map(install_json).collect::<Vec<_>>(),
    })
}

/// 体检：逐版本核对部署在位与登记基线（文件数），回结构化结果。
pub fn doctor_json(root: &Path) -> Value {
    let m = read_manifest(root);
    let mut checks = Vec::new();
    for i in &m.installed {
        let dir = version_dir(root, &i.version);
        let deployed = check_deployed(&dir).is_ok();
        let files = count_files(&dir).unwrap_or(0);
        checks.push(json!({
            "version": i.version,
            "deployed": deployed,
            "filesMatch": files == i.files,
            "files": files,
            "registered": i.files,
        }));
    }
    let pin_ok = m
        .pinned
        .as_ref()
        .is_some_and(|p| check_deployed(&version_dir(root, p)).is_ok());
    json!({
        "root": root.display().to_string(),
        "healthy": checks.iter().all(|c| c["deployed"] == json!(true))
            && (m.pinned.is_none() || pin_ok),
        "pinnedOk": if m.pinned.is_none() { Value::Null } else { json!(pin_ok) },
        "checks": checks,
    })
}

/// 托管位解析：pin 版本的 chrome 二进制路径（发现序的托管档）。
///
/// pin 未设、目录缺失或二进制不在（登记与盘面漂移）一律 `None`，
/// 退给后续发现序；漂移面由 [`doctor_json`] 报。
pub fn pinned_chrome(root: &Path) -> Option<PathBuf> {
    let m = read_manifest(root);
    let v = m.pinned?;
    cdp::spawn::chrome_binary_in_dir(&version_dir(root, &v))
}

fn count_files(dir: &Path) -> Option<u64> {
    let mut n = 0u64;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(d).ok()? {
            let entry = entry.ok()?;
            if entry.file_type().ok()?.is_dir() {
                stack.push(entry.path());
            } else {
                n += 1;
            }
        }
    }
    Some(n)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn install_json(i: &ChromeInstall) -> Value {
    json!({
        "version": i.version,
        "source": i.source,
        "files": i.files,
        "bytes": i.bytes,
        "installedAt": i.installed_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 独立临时根目录（测试自管生命周期）。
    fn tmp_root(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "browse-chrome-mgr-{tag}-{}-{}",
            std::process::id(),
            now_ms()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 造一个最小可用部署目录（一个假 chrome 二进制加一个附属文件）。
    fn fake_deploy(base: &Path) -> PathBuf {
        let d = base.join("chromium-1.2.3.4");
        std::fs::create_dir_all(d.join("sub")).unwrap();
        let bin = if cfg!(windows) {
            "chrome.exe"
        } else {
            "chrome"
        };
        std::fs::write(d.join(bin), b"bin").unwrap();
        std::fs::write(d.join("sub").join("data.pak"), b"x").unwrap();
        d
    }

    /// 版本号校验：合法通过，穿越与空串拒绝。
    #[test]
    fn version_guard() {
        assert!(valid_version("152.0.7977.84").is_ok());
        assert!(valid_version("a-b_1").is_ok());
        assert!(valid_version("").is_err());
        assert!(valid_version("../evil").is_err());
        assert!(valid_version("a/b").is_err());
    }

    /// 目录名版本提取：带 chromium- 前缀剥前缀，裸版本名原样。
    #[test]
    fn version_from_names() {
        assert_eq!(
            version_from_dir_name(Path::new("/x/chromium-152.0.7977.84")),
            "152.0.7977.84"
        );
        assert_eq!(version_from_dir_name(Path::new("/x/9.9")), "9.9");
    }

    /// 导入安装全链：复制+登记+自动 pin+托管位解析。
    #[test]
    fn install_pins_and_resolves() {
        let root = tmp_root("install");
        let src_base = tmp_root("src");
        let src = fake_deploy(&src_base);

        let brief = install_from_dir(&root, "1.2.3.4", &src).unwrap();
        assert_eq!(brief["version"], json!("1.2.3.4"));
        assert_eq!(brief["files"], json!(2));

        let m = read_manifest(&root);
        assert_eq!(m.pinned.as_deref(), Some("1.2.3.4"));
        assert_eq!(m.installed.len(), 1);

        let bin = pinned_chrome(&root).expect("托管位应解析到二进制");
        let name = bin.file_name().unwrap().to_str().unwrap().to_string();
        assert!(name.starts_with("chrome"));

        // 重复装同版本拒绝（不覆盖）
        assert!(install_from_dir(&root, "1.2.3.4", &src).is_err());

        std::fs::remove_dir_all(&root).ok();
        std::fs::remove_dir_all(&src_base).ok();
    }

    /// 源目录不是可用部署：拒绝并带 CTA。
    #[test]
    fn install_rejects_non_deploy() {
        let root = tmp_root("badsrc");
        let empty = tmp_root("empty");
        let err = install_from_dir(&root, "1.0", &empty).unwrap_err();
        assert!(format!("{err:#}").contains("chromeInstall"));
        std::fs::remove_dir_all(&root).ok();
        std::fs::remove_dir_all(&empty).ok();
    }

    /// pin 切换：装两版切来切去，未装版本拒绝并带 CTA。
    #[test]
    fn use_switches_between_installed() {
        let root = tmp_root("use");
        let base = tmp_root("use-src");
        let a = fake_deploy(&base);
        let b_dir = base.join("chromium-2.0.0.0");
        std::fs::create_dir_all(&b_dir).unwrap();
        let bin = if cfg!(windows) {
            "chrome.exe"
        } else {
            "chrome"
        };
        std::fs::write(b_dir.join(bin), b"bin2").unwrap();

        install_from_dir(&root, "1.2.3.4", &a).unwrap();
        install_from_dir(&root, "2.0.0.0", &b_dir).unwrap();
        assert_eq!(
            use_version(&root, "1.2.3.4").unwrap()["pinned"],
            json!("1.2.3.4")
        );
        assert_eq!(
            pinned_chrome(&root).unwrap(),
            version_dir(&root, "1.2.3.4").join(bin)
        );
        assert_eq!(
            use_version(&root, "2.0.0.0").unwrap()["pinned"],
            json!("2.0.0.0")
        );

        let err = use_version(&root, "9.9.9").unwrap_err();
        assert!(format!("{err:#}").contains("chromeInstall"));

        std::fs::remove_dir_all(&root).ok();
        std::fs::remove_dir_all(&base).ok();
    }

    /// doctor：盘面漂移（删二进制）报不健康，pin 失效时托管位退 None。
    #[test]
    fn doctor_catches_drift() {
        let root = tmp_root("doctor");
        let base = tmp_root("doctor-src");
        let src = fake_deploy(&base);
        install_from_dir(&root, "1.2.3.4", &src).unwrap();

        let ok = doctor_json(&root);
        assert_eq!(ok["healthy"], json!(true));

        let bin_name = if cfg!(windows) {
            "chrome.exe"
        } else {
            "chrome"
        };
        std::fs::remove_file(version_dir(&root, "1.2.3.4").join(bin_name)).unwrap();
        let bad = doctor_json(&root);
        assert_eq!(bad["healthy"], json!(false));
        assert_eq!(bad["pinnedOk"], json!(false));
        assert!(pinned_chrome(&root).is_none(), "漂移后退发现序");

        std::fs::remove_dir_all(&root).ok();
        std::fs::remove_dir_all(&base).ok();
    }

    /// 极简 HTTP 镜像 mock：路径精确匹配即 200，否则 404；逐请求一线程服务。
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

    /// 造一个部署形 zip（单顶层目录 chromium-<ver>/ 内含 chrome 二进制）加其 sha256。
    fn fake_zip_asset(version: &str) -> (Vec<u8>, String) {
        use std::io::Write;
        let bin = if cfg!(windows) {
            "chrome.exe"
        } else {
            "chrome"
        };
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opt = zip::write::SimpleFileOptions::default();
        for (name, data) in [
            (format!("chromium-{version}/{bin}"), b"bin".to_vec()),
            (format!("chromium-{version}/sub/data.pak"), b"x".to_vec()),
        ] {
            w.start_file(name, opt).unwrap();
            w.write_all(&data).unwrap();
        }
        let bytes = w.finish().unwrap().into_inner();
        let digest = format!("{:02x}", Sha256::digest(&bytes));
        (bytes, digest)
    }

    /// 镜像下载腿三态：happy（校验过、原子落位、登记 pin、零残目录）、
    /// 锚不匹配（错包即弃）、边车 404（CTA 带覆写指引）。
    /// 显式镜像参数直调内核（[`install_from_mirror_with`]），零 env 动作，并行安全。
    #[test]
    fn mirror_install_three_states() {
        let root = tmp_root("mirror");

        // happy：路由对上定标形 asset_name（显式镜像与资产名，零 env 动作）
        let (zip, digest) = fake_zip_asset("1.2.3.4");
        let asset = asset_name("1.2.3.4");
        let mirror = mock_mirror(vec![
            (
                format!("/1.2.3.4/{asset}.sha256"),
                format!("{digest}  {asset}").into_bytes(),
            ),
            (format!("/1.2.3.4/{asset}"), zip),
        ]);
        let brief = install_from_mirror_with(&root, "1.2.3.4", &mirror, &asset).unwrap();
        assert_eq!(brief["version"], json!("1.2.3.4"));
        assert_eq!(brief["files"], json!(2));
        assert_eq!(read_manifest(&root).pinned.as_deref(), Some("1.2.3.4"));
        assert!(pinned_chrome(&root).is_some(), "装完即 pin 生效");
        assert!(!root.join(".staging-1.2.3.4").exists(), "staging 用后即清");

        // 锚不匹配：错包即弃，无版本目录无残件
        let (zip2, _) = fake_zip_asset("2.0.0.0");
        let wrong = format!("{:064x}", 0u128); // 32 字节全零，长度对但值错
        let asset2 = asset_name("2.0.0.0");
        let mirror2 = mock_mirror(vec![
            (
                format!("/2.0.0.0/{asset2}.sha256"),
                format!("{wrong}  {asset2}").into_bytes(),
            ),
            (format!("/2.0.0.0/{asset2}"), zip2),
        ]);
        let err = install_from_mirror_with(&root, "2.0.0.0", &mirror2, &asset2).unwrap_err();
        assert!(format!("{err:#}").contains("sha256 不匹配"));
        assert!(!version_dir(&root, "2.0.0.0").exists());
        assert!(!root.join(".staging-2.0.0.0").exists());

        // 边车 404：CTA 带端点与 BROWSE_CHROME_ASSET 覆写指引
        let empty = mock_mirror(vec![]);
        let err =
            install_from_mirror_with(&root, "3.0.0.0", &empty, &asset_name("3.0.0.0")).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("404"), "404 应在错误里：{msg}");
        assert!(msg.contains("BROWSE_CHROME_ASSET"));
        assert!(!root.join(".staging-3.0.0.0").exists());

        std::fs::remove_dir_all(&root).ok();
    }

    /// 资产名是定标三元组形（总台裁二）：chromium-<版本>-<三元组>.zip。
    #[test]
    fn asset_name_is_triple_form() {
        let n = asset_name("152.0.7977.84");
        assert!(n.starts_with("chromium-152.0.7977.84-"), "前缀形：{n}");
        assert!(
            n.ends_with(&format!("{}.zip", chromium_triple())),
            "本平台三元组收尾：{n}"
        );
    }

    /// update 内核三态：已装最新只切 pin（不触镜像）、幂等重跑、previousPin 回填
    /// （未装走镜像腿，不入单测；那是 install_from_mirror 与本内核的组合）。
    #[test]
    fn update_switches_pin_when_installed() {
        let root = tmp_root("upd");
        let base = tmp_root("upd-src");
        let a = fake_deploy(&base);
        let b = fake_deploy(&base);

        install_from_dir(&root, "1.0.0.0", &a).unwrap();
        install_from_dir(&root, "2.0.0.0", &b).unwrap();
        use_version(&root, "1.0.0.0").unwrap();

        let brief = update_with(&root, "2.0.0.0").unwrap();
        assert_eq!(brief["version"], json!("2.0.0.0"));
        assert_eq!(
            brief["installedNow"],
            json!(false),
            "已装版本不得再触镜像腿"
        );
        assert_eq!(brief["previousPin"], json!("1.0.0.0"));
        assert_eq!(brief["pinned"], json!("2.0.0.0"));
        assert_eq!(read_manifest(&root).pinned.as_deref(), Some("2.0.0.0"));

        // 幂等：再 update 同版本，previousPin 即当前 pin
        let again = update_with(&root, "2.0.0.0").unwrap();
        assert_eq!(again["previousPin"], json!("2.0.0.0"));

        std::fs::remove_dir_all(&root).ok();
        std::fs::remove_dir_all(&base).ok();
    }

    /// remove 三态：删非 pin 版（目录加登记清、pin 不动、回释放量）、
    /// pin 指向的拒删、未装的拒删。
    #[test]
    fn remove_version_three_states() {
        let root = tmp_root("rm");
        let base = tmp_root("rm-src");
        let a = fake_deploy(&base);
        let b = fake_deploy(&base);

        install_from_dir(&root, "1.0.0.0", &a).unwrap();
        install_from_dir(&root, "2.0.0.0", &b).unwrap();
        use_version(&root, "1.0.0.0").unwrap();

        // pin 指向的拒删并带先切 CTA
        let err = remove_version(&root, "1.0.0.0").unwrap_err();
        assert!(
            format!("{err:#}").contains("chrome use"),
            "CTA 应指先切 pin：{err:#}"
        );

        // 删非 pin 版：目录没了、登记没了、pin 不动
        let brief = remove_version(&root, "2.0.0.0").unwrap();
        assert_eq!(brief["removed"], json!("2.0.0.0"));
        assert_eq!(brief["files"], json!(2), "fake_deploy 两文件");
        assert!(!version_dir(&root, "2.0.0.0").exists());
        let m = read_manifest(&root);
        assert_eq!(m.installed.len(), 1);
        assert_eq!(m.pinned.as_deref(), Some("1.0.0.0"), "pin 不动");

        // 未装的拒删并带已装清单
        let err = remove_version(&root, "9.9.9.9").unwrap_err();
        assert!(
            format!("{err:#}").contains("1.0.0.0"),
            "错误应带已装清单：{err:#}"
        );

        std::fs::remove_dir_all(&root).ok();
        std::fs::remove_dir_all(&base).ok();
    }
}
