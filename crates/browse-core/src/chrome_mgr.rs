//! 内嵌 Chromium 版本管理器（ADR-0007）：各版本 clean-chrome 在本仓应用
//! 数据目录下版本化管理（安装、pin、升级、体检）。
//!
//! 布局：`<state>/chromium/<version>/`（每版本一目录，旧版保留可回退），
//! 登记 `<state>/chromium/manifest.json`（已装版本 + pin 指向）。
//! 引擎 user-data 不在版本目录（engine-profile 跨版本持久，升级零迁移）。
//!
//! 安装源两形（ADR-0007）：本地目录导入（SxS 部署形态，`chromium-<ver>/`
//! 整目录复制；本实现面）；R2 镜像版本段下载（omc 分发面，端点未定标，
//! 接口在册待 omc 协调后补）。

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
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
    let bin = dir.join(cdp::spawn::chrome_binary_name());
    if bin.is_file() {
        Ok(())
    } else {
        bail!(
            "{} 不是可用的 Chromium 部署（{} 不在）；\
             下一步：chromeInstall({{fromDir: \"<部署目录>\"}}) 指向含 chrome 二进制的目录",
            dir.display(),
            bin.display()
        )
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
    let (files, bytes) = copy_tree(from_dir, &dst).map_err(|e| {
        anyhow::anyhow!(
            "复制部署失败（{} -> {}）：{e}",
            from_dir.display(),
            dst.display()
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
    let bin = version_dir(root, &v).join(cdp::spawn::chrome_binary_name());
    bin.is_file().then_some(bin)
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
}
