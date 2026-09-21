//! workspace 单仓管理（#50/#51 配套）：站点与机制知识仓
//! `github.com/raystyle/browse_workspace` 的安装、更新与仓内读取。
//!
//! 何时用：`browse workspace` 命令族的全部后端（status/install/update/
//! list/site/page）。git 维护走 shell-out `git`（不引 git crate）：install
//! 是 clone、update 是 `pull --ff-only`，本地修改可手工 commit/push 回推
//! 同一远端。仓根定位见 [`crate::paths::workspace_dir`]。

use anyhow::{Context, Result, anyhow, bail};
use serde_json::json;
use std::path::{Path, PathBuf};

/// 种子仓远端（install 的缺省源；本地修改 push 回推同一远端）。
pub const SEED_REMOTE: &str = "https://github.com/raystyle/browse_workspace.git";

/// 域名层点名清单的封顶（#50 验收：回执文件列表封顶 10；list 同口径）。
pub const DOMAIN_FILES_CAP: usize = 10;

/// git 子进程收口（`&str` 实参版）：见 [`git_os`]。
fn git(root: Option<&Path>, args: &[&str]) -> Result<String> {
    git_os(
        root,
        &args
            .iter()
            .map(|s| std::ffi::OsStr::new(*s))
            .collect::<Vec<_>>(),
    )
}

/// git 子进程收口（`&OsStr` 实参版，评审 G7）：`git --version` 或
/// `git -C <root> <args...>`（current_dir 代 -C，全程 argv 直传无 shell），
/// 取 stdout trim。内部函数，错误串自带下一步指令。root 与实参走
/// `OsStr` 保非 UTF-8 路径不经 lossy 替换（clone 目标错位的注入面）。
fn git_os(root: Option<&Path>, args: &[&std::ffi::OsStr]) -> Result<String> {
    let mut cmd = std::process::Command::new("git");
    if let Some(r) = root {
        cmd.current_dir(r);
    }
    cmd.args(args);
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    let out = cmd.output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            anyhow!("git 未安装或不在 PATH；下一步：装 git，或手工 git clone {SEED_REMOTE} 到仓根")
        } else {
            anyhow!("git 启动失败（{e}）；下一步：看 git 是否可用")
        }
    })?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let detail = if stderr.is_empty() { stdout } else { stderr };
        // 展示面拼接（非执行面）：OsStr 逐个 lossy 再连接
        let argv = args
            .iter()
            .map(|a| a.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ");
        bail!("git {argv} 失败（{detail}）；下一步：在仓根手工跑同一命令看完整输出");
    }
    Ok(stdout)
}

/// `browse workspace install`：git clone 种子仓到 `root`。clone 不带
/// `--depth`（shallow 与 `pull --ff-only`/回推有边角，仓极小不值得），
/// 分支吃缺省。回 `{root, remote, head}`。
///
/// # Errors
///
/// git 未安装；目录已被占用（存在且非空）；clone 非零退出（网络、权限）。
pub fn install(root: &Path) -> Result<serde_json::Value> {
    git(None, &["--version"]).context("git 预检")?;
    if root.exists() {
        let non_empty = std::fs::read_dir(root)
            .map(|mut it| it.next().is_some())
            .unwrap_or(true);
        if non_empty {
            bail!(
                "仓根 {} 已存在且非空；下一步：已装过就 browse workspace update，\
                 换路径就设 BROWSE_WORKSPACE",
                root.display()
            );
        }
    }
    if let Some(parent) = root.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("建父目录 {}", parent.display()))?;
    }
    git_os(
        None,
        &[
            std::ffi::OsStr::new("clone"),
            std::ffi::OsStr::new(SEED_REMOTE),
            root.as_os_str(),
        ],
    )?;
    let head = git(Some(root), &["rev-parse", "--short", "HEAD"]).unwrap_or_default();
    Ok(json!({
        "root": root.to_string_lossy(),
        "remote": SEED_REMOTE,
        "head": head,
    }))
}

/// `browse workspace update`：脏树先拒（本地修改未提交会挡 fast-forward），
/// 再 `git pull --ff-only`。回 `{root, head}`。
///
/// # Errors
///
/// 未安装；本地有未提交修改；git 缺失；pull 非零退出（远端分叉等）。
pub fn update(root: &Path) -> Result<serde_json::Value> {
    if !root.exists() {
        bail!(
            "仓根 {} 不存在；下一步：browse workspace install",
            root.display()
        );
    }
    let dirty = git(Some(root), &["status", "--porcelain"]).unwrap_or_default();
    if !dirty.is_empty() {
        bail!(
            "workspace 仓有未提交修改（{} 处）；下一步：cd {} 手工 commit/push 回推，\
             或 stash 后再 update",
            dirty.lines().count(),
            root.display()
        );
    }
    git(Some(root), &["pull", "--ff-only"])?;
    let head = git(Some(root), &["rev-parse", "--short", "HEAD"]).unwrap_or_default();
    Ok(json!({
        "root": root.to_string_lossy(),
        "head": head,
    }))
}

/// `browse workspace status` 的机器面：`{installed, root, gitPresent,
/// remote, branch, head, dirtyChanges, domainSites, domainFiles,
/// pageSlugs}`。git 子项失败容忍降级 null（status 是概览不是断言）。
///
/// # Errors
///
/// 仅当仓在位而 git 查询整体失败时仍返回概览（不报错）；本函数把
/// git 缺失也降级为 `gitPresent:false`，正常路径不报错。
pub fn status_json(root: &Path) -> Result<serde_json::Value> {
    let installed = root.exists();
    let git_present = git(None, &["--version"]).is_ok();
    let (remote, branch, head, dirty) = if installed && git_present {
        let r = git(Some(root), &["remote", "get-url", "origin"]).ok();
        let b = git(Some(root), &["rev-parse", "--abbrev-ref", "HEAD"]).ok();
        let h = git(Some(root), &["rev-parse", "--short", "HEAD"]).ok();
        let d = git(Some(root), &["status", "--porcelain"])
            .map(|s| s.lines().count() as u64)
            .unwrap_or(0);
        (r, b, h, d)
    } else {
        (None, None, None, 0)
    };
    let (domain_sites, domain_files, page_slugs) = skill_counts(root);
    Ok(json!({
        "installed": installed,
        "root": root.to_string_lossy(),
        "gitPresent": git_present,
        "remote": remote,
        "branch": branch,
        "head": head,
        "dirtyChanges": dirty,
        "domainSites": domain_sites,
        "domainFiles": domain_files,
        "pageSlugs": page_slugs,
    }))
}

/// `browse workspace list` 的机器面：`{domains: [{segment, files}],
/// pages: [slug]}`。files 与回执点名同口径封顶 [`DOMAIN_FILES_CAP`]；
/// 未安装返回空结构（由调用面给安装提示）。
pub fn list_json(root: &Path) -> serde_json::Value {
    let mut domains = Vec::new();
    if let Ok(rd) = std::fs::read_dir(root.join("domain-skills")) {
        let mut segs: Vec<String> = rd
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().to_str().map(str::to_string))
            .collect();
        segs.sort();
        for seg in segs {
            // capped 判据是「截断发生过」而非「恰好到帽」（评审 G6）：
            // 恰好 10 个文件不是「可能还有更多」
            let all = all_segment_files(&root.join("domain-skills").join(&seg));
            let capped = all.len() > DOMAIN_FILES_CAP;
            let mut files = all;
            files.truncate(DOMAIN_FILES_CAP);
            domains.push(json!({
                "segment": seg,
                "files": files,
                "capped": capped,
            }));
        }
    }
    let mut pages = Vec::new();
    if let Ok(rd) = std::fs::read_dir(root.join("page-skills")) {
        let mut slugs: Vec<String> = rd
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
            // README.md 是层索引不是配方（README 当 slug 会被读全文命令误下钻）
            .filter(|e| e.file_name() != "README.md")
            .filter_map(|e| {
                e.path()
                    .file_stem()
                    .and_then(|s| s.to_str().map(str::to_string))
            })
            .collect();
        slugs.sort();
        pages = slugs;
    }
    json!({ "domains": domains, "pages": pages })
}

/// 仓内相对读取的越界守卫：canonicalize 后必须仍在 `root` 内，挡绝对
/// 路径、`..` 与符号链接出仓（snippets #44 评审 F2 同律）。返回可读的
/// 规范路径。
fn guarded_join(root: &Path, rel: &str, what: &str) -> Result<PathBuf> {
    let joined = root.join(rel);
    let canon = joined.canonicalize().map_err(|_| {
        anyhow!(
            "{what} {rel} 不存在（{}）；下一步：browse workspace list 看在册条目",
            joined.display()
        )
    })?;
    let root_canon = root.canonicalize().map_err(|_| {
        anyhow!(
            "workspace 仓根 {} 不存在；下一步：browse workspace install",
            root.display()
        )
    })?;
    if !canon.starts_with(&root_canon) {
        bail!("{what} {rel} 越出 workspace 仓（只许仓内相对路径）");
    }
    Ok(canon)
}

/// `browse workspace site <段>[/<文件>]`：只给段时打该段清单（每行
/// `<文件>  <首行标题>`，与 snippets list 同构；**不封顶**，是回执点名
/// 的下钻面）；带 `<段>/<文件>` 时读该文件全文。
///
/// # Errors
///
/// 段或文件不存在（CTA 指回 list）；路径越出仓根；仓根未安装。
pub fn read_site(root: &Path, rel: &str) -> Result<String> {
    let seg_root = guarded_join(root, &format!("domain-skills/{rel}"), "workspace site")?;
    if seg_root.is_file() {
        return std::fs::read_to_string(&seg_root)
            .with_context(|| format!("读 {}", seg_root.display()));
    }
    let files = all_segment_files(&seg_root);
    if files.is_empty() {
        bail!(
            "workspace site 段 {rel} 不存在或为空（{}）；下一步：browse workspace list 看在册段",
            root.join("domain-skills").join(rel).display()
        );
    }
    let mut out = String::new();
    for f in files {
        let heading = first_heading(&seg_root.join(&f));
        out.push_str(&format!("{f}  {heading}\n"));
    }
    Ok(out)
}

/// `browse workspace page <slug>`：读 `page-skills/<slug>.md` 全文。
///
/// # Errors
///
/// slug 不存在（CTA 指回 list）；路径越出仓根；仓根未安装。
pub fn read_page(root: &Path, slug: &str) -> Result<String> {
    let path = guarded_join(root, &format!("page-skills/{slug}.md"), "workspace page")?;
    if !path.is_file() {
        bail!(
            "workspace page {slug} 不存在（{}）；下一步：browse workspace list 看在册 slug",
            path.display()
        );
    }
    std::fs::read_to_string(&path).with_context(|| format!("读 {}", path.display()))
}

/// 列 `<root>/domain-skills/<段>/` 的技能文件名（排序，封顶
/// [`DOMAIN_FILES_CAP`]，只收 .md/.txt）。目录缺失返回空。何时用：
/// 技能触发层（[`crate::skills`]）的点名清单与 `browse workspace list`
/// 共用此口径。
pub fn domain_segment_files(root: &Path, segment: &str) -> Vec<String> {
    let mut files = all_segment_files(&root.join("domain-skills").join(segment));
    files.truncate(DOMAIN_FILES_CAP);
    files
}

/// 同 [`domain_segment_files`] 但不封顶（site 清单的下钻面用）。
fn all_segment_files(dir: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<String> = rd
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|x| x == "md" || x == "txt")
        })
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .collect();
    files.sort();
    files
}

/// 文件首行 `#` 标题（无标题返回空串）。
fn first_heading(path: &Path) -> String {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| t.lines().find(|l| l.starts_with("# ")).map(str::to_string))
        .map(|l| l.trim_start_matches("# ").trim().to_string())
        .unwrap_or_default()
}

/// 仓内技能计数：`{段数, 域文件总数（不封顶，all 口径非 list 封顶）,
/// page slug 数}`（评审 G-C：status 概览不该跟着 list 的 10 帽少报）。
fn skill_counts(root: &Path) -> (u64, u64, u64) {
    let list = list_json(root);
    let sites = list
        .get("domains")
        .and_then(|d| d.as_array())
        .map_or(0, |d| d.len() as u64);
    let mut files = 0u64;
    if let Some(doms) = list.get("domains").and_then(|d| d.as_array()) {
        for d in doms {
            let seg = d
                .get("segment")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            files += all_segment_files(&root.join("domain-skills").join(seg)).len() as u64;
        }
    }
    let pages = list
        .get("pages")
        .and_then(|p| p.as_array())
        .map_or(0, |p| p.len() as u64);
    (sites, files, pages)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SEQ: AtomicUsize = AtomicUsize::new(0);

    /// 唯一 temp 仓：进程内递增防互撞，测后清理。
    fn temp_root(tag: &str) -> PathBuf {
        let n = SEQ.fetch_add(1, Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!("browse-ws-{}-{tag}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    fn seed(root: &Path) {
        std::fs::create_dir_all(root.join("domain-skills").join("x")).unwrap();
        std::fs::create_dir_all(root.join("page-skills")).unwrap();
        std::fs::write(
            root.join("domain-skills").join("x").join("a.md"),
            "# A 站要点\n正文\n",
        )
        .unwrap();
        std::fs::write(root.join("domain-skills").join("x").join("b.txt"), "纯文本").unwrap();
        std::fs::write(
            root.join("page-skills").join("captcha.md"),
            "# 验证码\n正文\n",
        )
        .unwrap();
    }

    /// list_json：段与 slug 齐全，文件排序，非 md/txt 被滤。
    #[test]
    fn list_shape() {
        let root = temp_root("list");
        seed(&root);
        let l = list_json(&root);
        let domains = l["domains"].as_array().unwrap();
        assert_eq!(domains.len(), 1);
        assert_eq!(domains[0]["segment"], json!("x"));
        assert_eq!(
            domains[0]["files"],
            json!(["a.md", "b.txt"]),
            "排序且过滤扩展名"
        );
        assert_eq!(l["pages"], json!(["captcha"]));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// list 封顶与分界（回执点名同口径；评审 G-D 锁「恰 10 不误标」）。
    #[test]
    fn list_caps_at_10() {
        let root = temp_root("cap");
        let seg = root.join("domain-skills").join("many");
        std::fs::create_dir_all(&seg).unwrap();
        for i in 0..11 {
            std::fs::write(seg.join(format!("f{i:02}.md")), "# f\n").unwrap();
        }
        // 恰 10 个文件的段：capped 必须 false（截断没发生过）
        let ten = root.join("domain-skills").join("ten");
        std::fs::create_dir_all(&ten).unwrap();
        for i in 0..10 {
            std::fs::write(ten.join(format!("t{i:02}.md")), "# t\n").unwrap();
        }
        let l = list_json(&root);
        let doms = l["domains"].as_array().unwrap();
        let many = doms.iter().find(|d| d["segment"] == json!("many")).unwrap();
        assert_eq!(many["files"].as_array().unwrap().len(), 10);
        assert_eq!(many["capped"], json!(true), "11 个文件截断发生过");
        let ten_d = doms.iter().find(|d| d["segment"] == json!("ten")).unwrap();
        assert_eq!(ten_d["files"].as_array().unwrap().len(), 10);
        assert_eq!(
            ten_d["capped"],
            json!(false),
            "恰 10 个文件不是可能还有更多"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// read_site：清单带首行标题；全文读取；未命中与越界都拒。
    #[test]
    fn read_site_paths() {
        let root = temp_root("site");
        seed(&root);
        let cat = read_site(&root, "x").unwrap();
        assert!(cat.contains("a.md  A 站要点"), "清单行含标题：{cat}");
        let full = read_site(&root, "x/a.md").unwrap();
        assert!(full.starts_with("# A 站要点"));
        assert!(read_site(&root, "nope").is_err(), "段未命中报错");
        assert!(
            read_site(&root, "../../../etc").is_err(),
            "越界拒绝（不存在路径同样失败）"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// read_site 绝对路径拒绝：join 到绝对路径会替换根，守卫必须拦下。
    #[test]
    fn read_site_absolute_rejected() {
        let root = temp_root("abs");
        seed(&root);
        let abs = if cfg!(windows) {
            "C:\\Windows\\win.ini"
        } else {
            "/etc/hostname"
        };
        assert!(read_site(&root, abs).is_err(), "绝对路径拒绝");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 符号链接出仓拒绝（POSIX；Windows 建链要特权不测）。
    #[cfg(unix)]
    #[test]
    fn read_site_symlink_rejected() {
        let root = temp_root("link");
        seed(&root);
        let outside = temp_root("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.md"), "# 秘密").unwrap();
        let seg = root.join("domain-skills").join("x");
        std::os::unix::fs::symlink(outside.join("secret.md"), seg.join("z.md")).unwrap();
        assert!(
            read_site(&root, "x/z.md").is_err(),
            "符号链接出仓拒绝（越界守卫）"
        );
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
    }

    /// read_page：命中读全文，未命中报错；仓内 `../` 语义（评审 G13 假绿
    /// 修正）：守卫只拦「出仓」，`page-skills/../README.md` 仍在仓内故放行
    /// （C8 定谳的设计语义，本测用存在目标锁边界，不再锁「文件不存在」）。
    #[test]
    fn read_page_paths() {
        let root = temp_root("page");
        seed(&root);
        assert!(read_page(&root, "captcha").unwrap().contains("验证码"));
        assert!(read_page(&root, "nope").is_err());
        std::fs::write(root.join("README.md"), "# 仓根 README\n").unwrap();
        assert!(
            read_page(&root, "../README")
                .unwrap()
                .contains("仓根 README"),
            "仓内 ../ 放行（只拦出仓）"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// status_json：git init 临时仓（离线）锁 installed 形——branch 有值、
    /// 无 origin 时 remote 为 null（不报错），计数正确（评审 G8 补单测）。
    #[test]
    fn status_git_repo_shape() {
        let root = temp_root("gitrepo");
        seed(&root);
        // init 加一条空 commit（裸 init 的 HEAD 未生，branch 查询恒失败）
        let ok = git(Some(&root), &["init", "-q", "-b", "main"]).is_ok()
            && git(
                Some(&root),
                &[
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@t",
                    "commit",
                    "--allow-empty",
                    "-q",
                    "-m",
                    "init",
                ],
            )
            .is_ok();
        if !ok {
            // git 缺失环境（理论仅极端 CI）：本测依赖 git，跳过而非假红
            let _ = std::fs::remove_dir_all(&root);
            return;
        }
        let s = status_json(&root).unwrap();
        assert_eq!(s["installed"], json!(true));
        assert_eq!(s["gitPresent"], json!(true));
        assert!(
            s["remote"].is_null(),
            "无 origin 时 remote 降级 null 非报错: {s}"
        );
        assert_eq!(s["branch"], json!("main"), "git 仓 branch 有值: {s}");
        assert!(s["head"].as_str().is_some(), "commit 后 head 有值: {s}");
        assert_eq!(s["domainSites"], json!(1));
        assert_eq!(s["pageSlugs"], json!(1));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// status_json：未安装时 installed=false 且计数为零，不报错。
    #[test]
    fn status_not_installed() {
        let root = temp_root("status");
        let s = status_json(&root).unwrap();
        assert_eq!(s["installed"], json!(false));
        assert_eq!(s["domainSites"], json!(0));
        let _ = std::fs::remove_dir_all(&root);
    }
}
