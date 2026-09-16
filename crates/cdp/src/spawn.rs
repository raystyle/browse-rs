//! 找到 Chrome 可执行文件并拉起一个带调试口的专属实例。
//!
//! 只服务于「引擎自起」路径：独立 user-data 目录、调试端口自动分配
//! （`--remote-debugging-port=0`，真实端口写进 profile 的
//! `DevToolsActivePort`）、`browse down` 时才终结，绝不碰用户默认 profile。

use anyhow::{Context, Result, anyhow};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::Duration;

/// 依优先序探测 Chrome 可执行文件，找不到返回 `None`。
///
/// 优先序：`BROWSE_CHROME` 环境变量 -> 可执行文件同目录与当前目录下的
/// `chromium-*/chrome.exe`（clean-chrome SxS 部署形态）-> 常规安装路径。
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// // 显式路径原样直通（不校验存在性）
/// assert_eq!(
///     cdp::spawn::find_chrome(Some(Path::new("/x/chrome"))),
///     Some("/x/chrome".into()),
/// );
/// // 无显式时按环境与部署探测，结果随机器而变，不在此断言
/// ```
pub fn find_chrome(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = explicit {
        return Some(p.to_path_buf());
    }
    if let Some(env) = std::env::var_os("BROWSE_CHROME") {
        let p = PathBuf::from(env);
        if p.is_file() {
            return Some(p);
        }
    }
    // 从 exe 目录与当前目录沿祖先向上爬，覆盖三种跑法：
    // 仓库内 cargo（target\debug\deps）、cargo test（cwd=包根）、装进 PATH 后在仓库里跑
    let mut roots = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        roots.push(dir.to_path_buf());
    }
    if let Ok(cwd) = std::env::current_dir() {
        roots.push(cwd);
    }
    for root in roots {
        for dir in root.ancestors().take(6) {
            if let Some(p) = newest_chromium_under(dir) {
                return chrome_binary_in_dir(&p);
            }
        }
    }
    fallback_paths().into_iter().find(|p| p.is_file())
}

fn newest_chromium_under(root: &Path) -> Option<PathBuf> {
    let mut hits: Vec<PathBuf> = std::fs::read_dir(root)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("chromium-"))
        })
        .filter(|p| chrome_binary_in_dir(p).is_some())
        .collect();
    hits.sort();
    hits.pop()
}

/// 返回本平台的 chrome 二进制名（Windows `chrome.exe`，其余 `chrome`）。
pub fn chrome_binary_name() -> &'static str {
    if cfg!(windows) {
        "chrome.exe"
    } else {
        "chrome"
    }
}

/// 在部署目录里解析 chrome 可执行文件（布局感知）：Windows `chrome.exe`、
/// Linux 裸 `chrome`、macOS `.app` 束内 `Chromium.app/Contents/MacOS/Chromium`
/// （mac 也接受裸 unix 形）。托管部署校验、pin 解析与祖先发现共用此口径。
///
/// # Examples
///
/// ```
/// # use std::path::Path;
/// // 目录里没有二进制时返回 None
/// assert!(cdp::spawn::chrome_binary_in_dir(Path::new("/nonexistent-dir")).is_none());
/// ```
pub fn chrome_binary_in_dir(dir: &Path) -> Option<PathBuf> {
    let mut candidates = vec![dir.join(chrome_binary_name())];
    if cfg!(target_os = "macos") {
        // 束在目录里（版本目录形）与目录本身是束（.app 直指形）都认
        candidates.push(
            dir.join("Chromium.app")
                .join("Contents")
                .join("MacOS")
                .join("Chromium"),
        );
        candidates.push(dir.join("Contents").join("MacOS").join("Chromium"));
    }
    candidates.into_iter().find(|p| p.is_file())
}

fn fallback_paths() -> Vec<PathBuf> {
    if cfg!(windows) {
        vec![
            PathBuf::from(r"C:\Program Files\Chromium\Application\chrome.exe"),
            PathBuf::from(r"C:\Program Files\Google\Chrome\Application\chrome.exe"),
        ]
    } else {
        vec![
            PathBuf::from("/usr/bin/chromium"),
            PathBuf::from("/Applications/Chromium.app/Contents/MacOS/Chromium"),
        ]
    }
}

/// 引擎 chrome 的 stdio 三路全显式：stderr 落 profile 旁的 `engine.log`，
/// stdin/stdout 置空。
///
/// 绝不继承父进程句柄；引擎常比单次调用方（CLI/测试）活得久，继承的
/// stderr 管道会让调用方管道永不 EOF（实测挂死过整条流水）。
fn engine_stdio(profile_dir: &Path) -> std::process::Stdio {
    let log = profile_dir
        .parent()
        .map(|p| p.join("engine.log"))
        .unwrap_or_else(|| PathBuf::from("engine.log"));
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
        .map(std::process::Stdio::from)
        .unwrap_or_else(|_| std::process::Stdio::null())
}

/// 拉起一个带调试口的专属引擎实例（独立 profile、端口自动分配）。
///
/// 参数：可执行文件、独立 profile 目录、是否无头。
///
/// 命令行：`--user-data-dir <dir> --remote-debugging-port=0 --no-first-run
/// --no-default-browser-check --no-sandbox [--headless] about:blank`。
///
/// `--no-sandbox`：SxS 部署的 clean-chrome 在本机沙箱进程打不开自身 exe
/// （`Sandbox cannot access executable`，0x5）；引擎是自动化专属隔离实例，
/// 不承载用户浏览面，关沙箱与 clean-chrome 验收实践一致（linux 同款）。
/// chrome 诊断写 `<state>/engine.log`。
///
/// # Errors
///
/// spawn 失败（路径不是可执行文件等）。
pub fn spawn_engine(chrome: &Path, profile_dir: &Path, headless: bool) -> Result<Child> {
    if let Some(parent) = profile_dir.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    // 清上次 run 的残留端口文件，防 wait 读到死端口（专属 profile，删除安全）
    std::fs::remove_file(profile_dir.join("DevToolsActivePort")).ok();
    let mut cmd = Command::new(chrome);
    cmd.arg(format!("--user-data-dir={}", profile_dir.display()))
        .arg("--remote-debugging-port=0")
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--no-sandbox")
        .arg("about:blank")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(engine_stdio(profile_dir));
    if headless {
        cmd.arg("--headless");
    }
    cmd.spawn()
        .with_context(|| format!("spawn {}", chrome.display()))
}

/// 等 spawn 出来的实例调试口就绪，返回 WS URL（读它 profile 下的
/// `DevToolsActivePort`）。
///
/// 冷启动首跑（建 profile）可能要几秒，默认上限 15 秒。
///
/// # Errors
///
/// 超时内端口文件没就绪。
pub async fn wait_devtools_ready(profile_dir: &Path) -> Result<String> {
    crate::discovery::wait_active_port_file(profile_dir, Duration::from_secs(15)).await
}

/// 管道态引擎的句柄：chrome 子进程加留在启动器侧的两条 CDP 管道端。
pub struct PipeEngine {
    /// chrome 子进程（drop 不杀；`Engine::shutdown` 负责）。
    pub child: Child,
    /// 从 chrome 读 CDP（out 管道的本侧）。
    pub read: crate::pipe::PipeReader,
    /// 往 chrome 写 CDP（in 管道的本侧）。
    pub write: crate::pipe::PipeWriter,
}

/// 管道通道的开关取值，即 `CLEAN_CHROME_DEBUG` 环境变量的值域。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipeMode {
    /// 只开 CDP 管道，不开 9222 端口。
    Pipe,
    /// 管道与 9222 端口双通道。
    Both,
}

impl PipeMode {
    /// 返回喂给 `CLEAN_CHROME_DEBUG` 的环境变量值。
    pub fn as_env(self) -> &'static str {
        match self {
            PipeMode::Pipe => "pipe",
            PipeMode::Both => "both",
        }
    }
}

/// 按管道契约拉起 clean-chrome（S005 / D02-6；Windows 句柄态与 POSIX
/// fd 3/4 布线两实现）。
///
/// 启动器建两条匿名管道，把「子读端,子写端」经
/// `--remote-debugging-io-pipes=<in>,<out>` 传入（十进制）。Windows：
/// 两枚句柄标可继承；POSIX：pre_exec 里 `dup2` 布到固定 fd 3/4（只用
/// async-signal-safe 的 dup2/close）后关原件。`CLEAN_CHROME_DEBUG=pipe|both`
/// 决定端口段开否。协议 ASCIIZ。断管即关浏览器（clean-chrome 行为），
/// 生命周期与本进程绑定。
///
/// # Errors
///
/// 建管道 / spawn 失败（POSIX：dup2 失败进 pre_exec 错误）。
pub fn spawn_engine_pipes(
    chrome: &Path,
    profile_dir: &Path,
    headless: bool,
    mode: PipeMode,
) -> Result<PipeEngine> {
    #[cfg(unix)]
    {
        use crate::pipe::anon_pair;
        use std::os::unix::process::CommandExt;

        if let Some(parent) = profile_dir.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        // 对齐 Windows 侧：child 读 in、写 out；本侧持有 in_w / out_r
        let (in_r, in_w) = anon_pair().context("pipe(in)")?;
        let (out_r, out_w) = anon_pair().context("pipe(out)")?;
        let in_fd = in_r.h as i32;
        let out_fd = out_w.h as i32;

        let mut cmd = Command::new(chrome);
        cmd.arg(format!("--user-data-dir={}", profile_dir.display()))
            .arg("--remote-debugging-io-pipes=3,4")
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .arg("--no-sandbox")
            .arg("about:blank")
            .env("CLEAN_CHROME_DEBUG", mode.as_env())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(engine_stdio(profile_dir));
        if headless {
            cmd.arg("--headless");
        }
        // fork 后 exec 前：把两端布到固定 fd 3/4（clean-chrome POSIX 契约）。
        // 闭包里只有 dup2/close（async-signal-safe）
        unsafe {
            cmd.pre_exec(move || {
                // 外层 unsafe 已覆盖（edition 2024 的 unsafe_op_in_unsafe_fn 不适用于
                // 闭包体内的调用，这里嵌套块会被 clippy 判多余）
                let wire = |src: i32, dst: i32| -> std::io::Result<()> {
                    if src == dst {
                        return Ok(());
                    }
                    if libc::dup2(src, dst) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    libc::close(src);
                    Ok(())
                };
                wire(in_fd, 3)?;
                wire(out_fd, 4)?;
                Ok(())
            });
        }
        let child = cmd
            .spawn()
            .with_context(|| format!("spawn {} (pipes fd3/4)", chrome.display()))?;
        // 子进程已持有 3/4；本侧关掉子侧副本防泄漏
        drop(in_r);
        drop(out_w);
        Ok(PipeEngine {
            child,
            read: out_r,
            write: in_w,
        })
    }
    #[cfg(windows)]
    {
        use crate::pipe::anon_pair;
        use std::os::windows::io::AsRawHandle;

        if let Some(parent) = profile_dir.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        // 对齐 pipe-smoke.py：child 读 in_r、写 out_w；本侧持有 in_w / out_r
        let (in_r, in_w) = anon_pair().context("pipe(in)")?;
        let (out_r, out_w) = anon_pair().context("pipe(out)")?;

        crate::pipe::set_inheritable(in_r.h, true)?;
        crate::pipe::set_inheritable(out_w.h, true)?;

        let mut cmd = Command::new(chrome);
        cmd.arg(format!("--user-data-dir={}", profile_dir.display()))
            .arg(format!(
                "--remote-debugging-io-pipes={},{}",
                in_r.as_raw_handle() as usize,
                out_w.as_raw_handle() as usize
            ))
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .arg("--no-sandbox")
            .arg("about:blank")
            .env("CLEAN_CHROME_DEBUG", mode.as_env())
            // chrome 诊断进 <state>/engine.log；不继承本进程句柄
            // （stderr(Stdio::inherit) 触发 bInheritHandles=TRUE 路径，
            // 但继承的 stderr 管道会让调用方流水永不 EOF）
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(engine_stdio(profile_dir));
        if headless {
            cmd.arg("--headless");
        }
        let child = cmd
            .spawn()
            .with_context(|| format!("spawn {} (pipes)", chrome.display()))?;
        // 子进程已收养两端（继承副本）；本侧关掉子侧副本防泄漏
        drop(in_r);
        drop(out_w);
        Ok(PipeEngine {
            child,
            read: out_r,
            write: in_w,
        })
    }
}

/// 强杀进程树（优雅退出 `Browser.close` 失败后的兜底）。
///
/// # Errors
///
/// Windows `taskkill` 或 unix `kill` 失败。
pub fn terminate_pid(pid: u32) -> Result<()> {
    let st = if cfg!(windows) {
        Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
    } else {
        Command::new("kill")
            .args(["-9", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
    };
    match st {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(anyhow!("terminate {pid}: exit {}", s.code().unwrap_or(-1))),
        Err(e) => Err(anyhow!("terminate {pid}: {e}")),
    }
}
