//! 引擎策略：附着优先，缺则自起（ADR-0003）。
//!
//! clean-chrome 专为自动化而生（`--auto-allow-devtools-connections` 免确认、
//! 无参数启动即开 9222），所以：
//!
//! 1. 显式 `ws` / `port`（CLI 旗标或 `BROWSE_CDP_WS`）优先直连。
//! 2. 否则探测本机已开的调试口（`/json/version`@9222 -> 默认 profile 的
//!    `DevToolsActivePort`），命中即附着；人机共存，绝不关用户的浏览器。
//! 3. 都没有就 spawn 专属实例：独立 profile、`--remote-debugging-port=0`、
//!    可 `--headless`。[`Engine::shutdown`] 只终结自己 spawn 的（优雅
//!    `Browser.close` -> 兜底杀进程树）。
//!
//! 连上后自动 attach 首个 page target（没有就开 about:blank），
//! 让 agent 一条命令即可 `session.Page.navigate(...)`（ADR-0004）；
//! 片段里的显式 `session.connect` / `session.use` 仍然可覆盖。

use anyhow::{Context, Result, anyhow};
use cdp::{ConnectOptions, Session, discovery, spawn as cdp_spawn};
use serde::Serialize;
use serde_json::json;
use std::path::PathBuf;
use std::process::Child;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// 引擎指令意图，由 CLI 旗标或环境变量解析而来。
#[derive(Debug, Clone)]
pub enum EngineSpec {
    /// 显式 WS URL 直连（`--ws` / `BROWSE_CDP_WS`）。
    Attach {
        /// WebSocket 端点。
        ws_url: String,
    },
    /// 显式端口（`--port`），走 `/json/version`。
    Port(
        /// 调试端口。
        u16,
    ),
    /// 自动策略：先探测附着，缺则 spawn（`--chrome` / `--headless` / `--pipe` /
    /// `--profile` / `--proxy` / `--isolated` 可约束 spawn 面）。
    Auto {
        /// 指定 chrome 可执行文件（`None` 走发现序）。
        chrome: Option<PathBuf>,
        /// spawn 时无头。
        headless: bool,
        /// spawn 走 CDP 管道通道（`CLEAN_CHROME_DEBUG=pipe`，不开 9222）。
        pipe: bool,
        /// spawn 引擎的自定义 profile（user-data-dir；`None` 用默认固定
        /// `<state>/engine-profile`，持久保存站点状态与会话）。
        profile: Option<PathBuf>,
        /// 引擎代理（`--proxy-server`，#25.5）：`BROWSE_PROXY` 同值。
        proxy: Option<String>,
        /// 代理旁路清单（`--proxy-bypass-list`，#25.5）：`BROWSE_PROXY_BYPASS`。
        proxy_bypass: Option<String>,
        /// 隔离态 profile（#25.3 拆出）：引擎退出即删 profile 目录，
        /// 不留站点痕迹；目录由引擎自管。
        isolated: bool,
        /// 引擎附加旗标（#48）：`--engine-arg` / `BROWSE_ENGINE_ARGS`
        /// 而来，spawn 时按 argv 元素原样直通（如
        /// `--enable-features=CleanChromeBrowseDomain` 开 clean-chrome
        /// 扩展域）。
        engine_args: Vec<String>,
    },
}

impl EngineSpec {
    /// 从 spec 派生 chrome 附加旗标（#25.5/#25.3/#48）：代理与旁路直通
    /// chrome 语义，引擎附加旗标原样透传；单测锁形。
    ///
    /// 纪律：值按单 flag 直通、不做转义或拆分（`cmd.args` 每项是一个
    /// argv 元素，无 shell 无空白拆分，无注入面）；新增字段一律经本函数
    /// 并补单测，别在这里加拼接/拆分逻辑。
    pub(crate) fn spawn_extra_args(&self) -> Vec<String> {
        let EngineSpec::Auto {
            proxy,
            proxy_bypass,
            engine_args,
            ..
        } = self
        else {
            return Vec::new();
        };
        let mut args = Vec::new();
        if let Some(p) = proxy {
            args.push(format!("--proxy-server={p}"));
        }
        if let Some(b) = proxy_bypass {
            args.push(format!("--proxy-bypass-list={b}"));
        }
        args.extend(engine_args.iter().cloned());
        args
    }
}

impl EngineSpec {
    /// 从环境解析缺省意图：`BROWSE_CDP_WS` 显式直连，否则自动策略。
    ///
    /// # Examples
    ///
    /// ```
    /// # use browse_core::engine::{EngineSpec, EngineSpec::*};
    /// // 未设 BROWSE_CDP_WS 时是自动策略
    /// if std::env::var_os("BROWSE_CDP_WS").is_none() {
    ///     assert!(matches!(EngineSpec::from_env(None, false, false), Auto { .. }));
    /// }
    /// ```
    pub fn from_env(chrome: Option<PathBuf>, headless: bool, pipe: bool) -> Self {
        if let Some(ws) = std::env::var_os("BROWSE_CDP_WS") {
            return EngineSpec::Attach {
                ws_url: ws.to_string_lossy().into_owned(),
            };
        }
        EngineSpec::Auto {
            chrome,
            headless,
            pipe,
            profile: std::env::var_os("BROWSE_PROFILE").map(Into::into),
            proxy: std::env::var("BROWSE_PROXY").ok(),
            proxy_bypass: std::env::var("BROWSE_PROXY_BYPASS").ok(),
            isolated: false,
            engine_args: std::env::var("BROWSE_ENGINE_ARGS")
                .map(|s| s.split_whitespace().map(str::to_string).collect())
                .unwrap_or_default(),
        }
    }
}

/// 引擎现状的可序列化快照，`/health` 面直接用。
#[derive(Debug, Clone, Serialize)]
pub enum EngineSource {
    /// 未连接。
    NotConnected,
    /// 附着了外部浏览器（绝不终结它）。
    Attached {
        /// 附着用的 WS 端点（或来源描述）。
        ws_url: String,
    },
    /// 自己 spawn 的专属实例（`browse down` 会终结）。
    Spawned {
        /// 端口态的 WS 端点（`close` 后重连用）；管道态为 `None`
        /// （断管即浏览器自亡，只能重 spawn）。
        ws_url: Option<String>,
        /// chrome 进程 pid。
        pid: u32,
        /// 独立 profile 目录。
        profile_dir: PathBuf,
        /// chrome 可执行文件。
        chrome: PathBuf,
        /// 是否无头。
        headless: bool,
        /// CDP 通道：`pipe`（S005 管道契约）或 `port`。
        channel: &'static str,
        /// spawn 时刻（Unix 毫秒，#59）：引擎所有权时间锚，status 的
        /// `engineProvenance.spawnedAt` 同源。
        spawned_at: u64,
    },
}

/// Unix 毫秒钟（#59）：`SystemTime` 取现值，取不到回 0（时间锚缺失比
/// 崩溃面轻）。
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// #59 引擎来源与所有权语义面（纯函数）：[`EngineSource`] 语义化为
/// `{origin, hostContext, spawnedBy, spawnedAt}` 四字段。何时用：status
/// 与 /health 的 `engineProvenance` 键；#60 操作回执告警面同源消费同一
/// 结构（两处展示一份事实）。边界：spawned 态引擎与 daemon 同宿主，
/// hostContext 是权威标注（daemon 的 os/hostname）；attached 态只能取
/// ws_url 的 host 字面（loopback 经隧道转发时真实宿主不可辨，显式降级
/// 标注不冒充确定）；未连接态四字段在但 origin 为 `none`。
///
/// # Examples
///
/// ```
/// # use browse_core::engine::{EngineSource, provenance};
/// let s = EngineSource::Spawned {
///     ws_url: None, pid: 42, profile_dir: "/p".into(), chrome: "/c".into(),
///     headless: true, channel: "port", spawned_at: 1700000000000,
/// };
/// let p = provenance(&s, false, "linux", "nuc-a");
/// assert_eq!(p["origin"], "managed-spawn");
/// assert_eq!(p["hostContext"], "linux/nuc-a");
/// assert_eq!(p["spawnedAt"], 1_700_000_000_000u64);
/// let a = EngineSource::Attached { ws_url: "ws://127.0.0.1:9222/x".into() };
/// let p = provenance(&a, false, "windows", "host-pc");
/// assert_eq!(p["origin"], "attached");
/// assert!(p["hostContext"].as_str().unwrap().contains("loopback"));
/// ```
pub fn provenance(
    source: &EngineSource,
    isolated: bool,
    daemon_os: &str,
    daemon_host: &str,
) -> serde_json::Value {
    match source {
        EngineSource::NotConnected => json!({
            "origin": "none",
            "hostContext": format!("{daemon_os}/{daemon_host}"),
            "spawnedBy": serde_json::Value::Null,
            "spawnedAt": serde_json::Value::Null,
        }),
        EngineSource::Attached { ws_url } => {
            // 附着态的宿主只能取 ws 字面：loopback（127.0.0.1/localhost/
            // ::1）可能是本机也可能是隧道转发，显式降级不冒充确定
            let host = ws_authority_host(ws_url);
            let ctx = if host.starts_with("127.")
                || host.starts_with("[::1")
                || host == "localhost"
                || host == "[::1]"
            {
                format!("{host}（loopback：本机或隧道转发不可辨）")
            } else {
                host
            };
            json!({
                "origin": "attached",
                "hostContext": ctx,
                "spawnedBy": "外部浏览器",
                "spawnedAt": serde_json::Value::Null,
            })
        }
        EngineSource::Spawned { spawned_at, .. } => json!({
            "origin": if isolated { "isolated-spawn" } else { "managed-spawn" },
            "hostContext": format!("{daemon_os}/{daemon_host}"),
            "spawnedBy": "daemon",
            "spawnedAt": spawned_at,
        }),
    }
}

/// #60 操作时刻引擎指纹（纯函数）：可比对的身份面——origin、pid、
/// 有头无头、通道、profile 形态、附着 host 字面。hostContext 与实例名
/// 在 daemon 生命周期内恒定不参与逐操作比对（跨宿主归 CLI 侧判）。
pub fn fingerprint_of(source: &EngineSource, isolated: bool) -> serde_json::Value {
    match source {
        EngineSource::NotConnected => json!({ "origin": "none" }),
        EngineSource::Attached { ws_url } => json!({
            "origin": "attached",
            "attachedHost": ws_authority_host(ws_url),
        }),
        EngineSource::Spawned {
            pid,
            headless,
            channel,
            ..
        } => json!({
            "origin": if isolated { "isolated-spawn" } else { "managed-spawn" },
            "pid": pid,
            "headless": headless,
            "channel": channel,
            "profileForm": if isolated { "isolated" } else { "persistent" },
        }),
    }
}

/// #60 指纹比对（纯函数）：两次操作的指纹差异列人读变化句；空 vec 即
/// 无变化（正常态安静）。字段缺失（形态切换后键集不同）按「有 vs 无」
/// 出变化句，不 panic。
pub fn fingerprint_changes(old: &serde_json::Value, new: &serde_json::Value) -> Vec<String> {
    let mut out = Vec::new();
    let g = |v: &serde_json::Value, k: &str| v.get(k).cloned().unwrap_or(serde_json::Value::Null);
    let oo = g(old, "origin").to_string();
    let no = g(new, "origin").to_string();
    if oo != no {
        // G2：origin 也是字符串，走裸值不带 JSON 引号
        fn bare(v: &str) -> &str {
            v.trim_matches('"')
        }
        out.push(format!("来源变化：{} -> {}", bare(&oo), bare(&no)));
    }
    // G1（评审）：origin 变化时键集必不同（附着与 spawn 指纹键不一样），
    // 存在性差异是形态切换副产物不是独立信号（附着态没有有头无头概念），
    // 只出来源句不稀释真信号；同 origin 下逐键比对才出独立句
    if oo != no {
        return out;
    }
    for k in ["pid", "headless", "channel", "profileForm", "attachedHost"] {
        let a = g(old, k);
        let b = g(new, k);
        if a != b && !(a.is_null() && b.is_null()) {
            let label = match k {
                "pid" => "引擎换新",
                "headless" => "有头无头翻转",
                "channel" => "通道变化",
                "profileForm" => "形态翻转（持久/隔离）",
                _ => "附着 host 变化",
            };
            // G2（评审）：字符串走 as_str 去引号与裸值风格一致；同 origin
            // 下键集恒同，缺失态不进
            let f = |v: serde_json::Value| {
                if let Some(b) = v.as_bool() {
                    // headless 语义：true 是无头
                    if b {
                        "无头".to_string()
                    } else {
                        "有头".to_string()
                    }
                } else if let Some(s) = v.as_str() {
                    s.to_string()
                } else {
                    v.to_string()
                }
            };
            out.push(format!("{label}：{} -> {}", f(a), f(b)));
        }
    }
    out
}

/// ws(s) URL 的 authority host 字面（#59）：剥 scheme 后取 `/` 或 `:`
/// 前段；带括号 IPv6 authority（`[::1]` 形）取首个 `]` 含括号整段
/// （评审 F2：IPv6 字面自带 `:` 不能按 `:` 切）；畸形串原样返回
/// （只做展示，不参与决策）。
fn ws_authority_host(ws_url: &str) -> String {
    let rest = ws_url
        .strip_prefix("ws://")
        .or_else(|| ws_url.strip_prefix("wss://"))
        .unwrap_or(ws_url);
    // IPv6 形（评审 F2）：']' 后面是 ':port'，host 含括号整段（IPv6
    // 字面自带 ':'，不能按 ':' 切）
    if rest.starts_with('[') {
        return rest
            .split(']')
            .next()
            .map(|h| format!("{h}]"))
            .unwrap_or_else(|| rest.to_string());
    }
    rest.split(['/', ':']).next().unwrap_or(rest).to_string()
}

/// 引擎状态机：确保连接、报告来源、只终结自己 spawn 的。
pub struct Engine {
    session: Arc<Session>,
    inner: Mutex<EngineInner>,
}

struct EngineInner {
    source: EngineSource,
    child: Option<Child>,
    /// 隔离态 profile 目录（#25.3 拆出）：shutdown 后删除，不留站点痕迹。
    isolated_dir: Option<PathBuf>,
}

/// 引擎 chrome 解析序（ADR-0007）：CLI 显式 `--chrome` 最先；`BROWSE_CHROME`
/// 环境变量交给 [`cdp::spawn::find_chrome`] 自查；两者皆空时托管 pin 版本
/// （`<state>/chromium/<pinned>/`）作为显式路径顶上，再往后是祖先部署与常规路径。
fn resolve_chrome(explicit: Option<PathBuf>) -> Option<PathBuf> {
    if explicit.is_some() || std::env::var_os("BROWSE_CHROME").is_some() {
        return explicit;
    }
    crate::chrome_mgr::pinned_chrome(&crate::chrome_mgr::chromium_root())
}

impl Engine {
    /// 绑定一条会话建引擎；冷态是 [`EngineSource::NotConnected`]。
    ///
    /// # Examples
    ///
    /// ```
    /// let engine = browse_core::Engine::new(cdp::Session::new());
    /// assert!(!engine.session().is_connected());
    /// ```
    pub fn new(session: Arc<Session>) -> Arc<Self> {
        Arc::new(Self {
            session,
            inner: Mutex::new(EngineInner {
                source: EngineSource::NotConnected,
                child: None,
                isolated_dir: None,
            }),
        })
    }

    /// 返回引擎绑定的共享会话（clone `Arc` 同一条连接）。
    pub fn session(&self) -> Arc<Session> {
        self.session.clone()
    }

    /// 返回当前引擎来源的快照。
    pub async fn source(&self) -> EngineSource {
        self.inner.lock().await.source.clone()
    }

    /// 把引擎带到在线状态（幂等：已连接直接返回现状）。
    ///
    /// # Errors
    ///
    /// - 显式 WS/端口连不上。
    /// - 自动策略下探测不到、且 chrome 找不到或 spawn 后 15 秒内调试口未就绪。
    /// - 连上后 attach 首个 page target 失败。
    pub async fn ensure(&self, spec: &EngineSpec) -> Result<EngineSource> {
        {
            let inner = self.inner.lock().await;
            if self.session.is_connected() {
                return Ok(inner.source.clone());
            }
        }
        // 重连快路径：自己 spawn 的端口态引擎还活着（session.close 后的恢复），
        // 直接重连同一实例，别丢下孤儿再开新的
        let reconnect_ws = match self.source().await {
            EngineSource::Spawned {
                ws_url: Some(ws), ..
            } => Some(ws),
            _ => None,
        };
        if let Some(ws) = reconnect_ws
            && self
                .session
                .connect_opts(ConnectOptions {
                    ws_url: Some(ws),
                    ..Default::default()
                })
                .await
                .is_ok()
        {
            self.attach_first_page().await?;
            return Ok(self.source().await);
        }
        // 到这里还挂着旧 spawn 状态（端口态连不上或管道态已亡）：先清尸再走新流程，
        // 否则 child 句柄被覆盖丢弃，浏览器成孤儿
        if matches!(self.source().await, EngineSource::Spawned { .. }) {
            let _ = self.shutdown().await;
        }
        let source = match spec {
            EngineSpec::Attach { ws_url } => {
                self.session
                    .connect_opts(ConnectOptions {
                        ws_url: Some(ws_url.clone()),
                        ..Default::default()
                    })
                    .await
                    .with_context(|| format!("attach {ws_url}"))?;
                EngineSource::Attached {
                    ws_url: ws_url.clone(),
                }
            }
            EngineSpec::Port(port) => {
                self.session
                    .connect_opts(ConnectOptions {
                        port: Some(*port),
                        ..Default::default()
                    })
                    .await
                    .with_context(|| format!("attach port {port}"))?;
                EngineSource::Attached {
                    ws_url: format!("port {port}"),
                }
            }
            EngineSpec::Auto {
                chrome,
                headless,
                pipe,
                profile,
                proxy: _,
                proxy_bypass: _,
                isolated,
                engine_args: _,
            } => {
                let extra = spec.spawn_extra_args();
                // 隔离态（#25.3 拆出）：spawn 才落 isolated-* 目录（附着探测
                // 分支不建目录）；退场即删
                let profile = if *isolated {
                    Some(crate::paths::state_dir().join(format!(
                        "isolated-{}",
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis())
                            .unwrap_or(0)
                    )))
                } else {
                    profile.clone()
                };
                // BROWSE_NO_ATTACH=1：跳过附着探测，强制 spawn 隔离实例
                // （测试确定性 / 「别碰我正开着的浏览器」的显式意图）
                let no_attach =
                    std::env::var_os("BROWSE_NO_ATTACH").is_some_and(|v| v == "1" || v == "true");
                if !no_attach && let Some(ws) = discovery::probe_default().await {
                    self.session
                        .connect_opts(ConnectOptions {
                            ws_url: Some(ws.clone()),
                            ..Default::default()
                        })
                        .await
                        .context("attach 已探测到的浏览器")?;
                    EngineSource::Attached { ws_url: ws }
                } else if *pipe {
                    return self
                        .spawn_pipes(
                            chrome.clone(),
                            *headless,
                            profile.clone(),
                            &extra,
                            *isolated,
                        )
                        .await;
                } else {
                    // spawn 分支自己落状态（含 child 句柄）并 attach，提前返回
                    return self
                        .spawn(
                            chrome.clone(),
                            *headless,
                            profile.clone(),
                            &extra,
                            *isolated,
                        )
                        .await;
                }
            }
        };
        *self.inner.lock().await = EngineInner {
            source: source.clone(),
            child: None,
            isolated_dir: None,
        };
        self.attach_first_page().await?;
        Ok(source)
    }

    /// spawn 端口态引擎；`profile` 为自定义 user-data-dir（`None` 用默认
    /// 固定 `<state>/engine-profile`，站点状态与会话跨跑持久）。
    async fn spawn(
        &self,
        chrome: Option<PathBuf>,
        headless: bool,
        profile: Option<PathBuf>,
        extra_args: &[String],
        isolated: bool,
    ) -> Result<EngineSource> {
        let chrome = cdp_spawn::find_chrome(resolve_chrome(chrome).as_deref()).ok_or_else(|| {
            anyhow!(
                "browse: 找不到 chrome；下一步：browse chrome install <版本> <部署目录>（或 --chrome <path> / BROWSE_CHROME 显式指定）"
            )
        })?;
        let profile = profile.unwrap_or_else(crate::paths::engine_profile_dir);
        let child = cdp_spawn::spawn_engine(&chrome, &profile, headless, extra_args)?;
        let pid = child.id();
        // spawn 之后的任何失败都必须杀掉 child：std Child 的 Drop 不杀进程，
        // 丢句柄 = 孤儿 chrome 锁死 profile，后续 spawn 全挂
        macro_rules! bail_kill {
            ($e:expr) => {{
                let _ = cdp_spawn::terminate_pid(pid);
                return Err($e);
            }};
        }
        let ws = match cdp_spawn::wait_devtools_ready(&profile).await {
            Ok(ws) => ws,
            Err(e) => bail_kill!(e.context(format!("spawn {} 后等调试口", chrome.display()))),
        };
        if let Err(e) = self
            .session
            .connect_opts(ConnectOptions {
                ws_url: Some(ws.clone()),
                ..Default::default()
            })
            .await
        {
            bail_kill!(e.context("连接自起引擎"));
        }
        let isolated_dir = isolated.then(|| profile.clone());
        *self.inner.lock().await = EngineInner {
            source: EngineSource::Spawned {
                ws_url: Some(ws),
                pid,
                profile_dir: profile,
                chrome,
                headless,
                channel: "port",
                spawned_at: now_ms(),
            },
            isolated_dir,
            child: Some(child),
        };
        // spawn 分支自己 attach（ensure 的统一 attach 拿不到 child 句柄归属）
        if let Err(e) = self.attach_first_page().await {
            let _ = self.shutdown().await;
            return Err(e);
        }
        Ok(self.source().await)
    }

    /// 管道态 spawn（S005 契约）：免端口探测、零 TCP 面、断管即关浏览器；
    /// `profile` 语义同 [`Engine::spawn`]。
    async fn spawn_pipes(
        &self,
        chrome: Option<PathBuf>,
        headless: bool,
        profile: Option<PathBuf>,
        extra_args: &[String],
        isolated: bool,
    ) -> Result<EngineSource> {
        let chrome = cdp_spawn::find_chrome(resolve_chrome(chrome).as_deref()).ok_or_else(|| {
            anyhow!(
                "browse: 找不到 chrome；下一步：browse chrome install <版本> <部署目录>（或 --chrome <path> / BROWSE_CHROME 显式指定）"
            )
        })?;
        let profile = profile.unwrap_or_else(crate::paths::engine_profile_dir);
        let engine = cdp_spawn::spawn_engine_pipes(
            &chrome,
            &profile,
            headless,
            cdp_spawn::PipeMode::Pipe,
            extra_args,
        )
        .context("管道态 spawn")?;
        let pid = engine.child.id();
        self.session
            .connect_pipes(engine.read, engine.write)
            .await?;
        let isolated_dir = isolated.then(|| profile.clone());
        *self.inner.lock().await = EngineInner {
            source: EngineSource::Spawned {
                ws_url: None,
                pid,
                profile_dir: profile,
                chrome,
                headless,
                channel: "pipe",
                spawned_at: now_ms(),
            },
            isolated_dir,
            child: Some(engine.child),
        };
        if let Err(e) = self.attach_first_page().await {
            let _ = self.shutdown().await;
            return Err(e);
        }
        Ok(self.source().await)
    }

    /// attach 首个 page target；没有真实页面就开一个 about:blank
    /// （对齐「ensure_real_tab」契约：不附着空目标）。
    async fn attach_first_page(&self) -> Result<()> {
        let tabs = self.session.list_page_targets().await?;
        let target = match tabs.first() {
            Some(t) => t.target_id.clone(),
            None => self.session.create_target("about:blank").await?,
        };
        let sid = self.session.use_target(&target).await?;
        // Page 域同步开（#19）：首用求值路径的立即 waitFor 不再竞开域时序
        crate::js_host::ensure_page_enabled(&self.session, &sid).await;
        Ok(())
    }

    /// 新开 about:blank tab 并设为活动路由（`--new-tab` 面）。
    ///
    /// # Errors
    ///
    /// 未连接（先 [`Engine::ensure`]）或 createTarget 失败。
    pub async fn new_tab(&self) -> Result<String> {
        if !self.session.is_connected() {
            return Err(anyhow!(
                "browse: 引擎未连接；先 browse up 或直接跑片段（自动拉起）"
            ));
        }
        let id = self.session.create_target("about:blank").await?;
        let sid = self.session.use_target(&id).await?;
        // Page 域同步开（#19）：--new-tab 后的立即 waitFor 不再竞开域时序
        crate::js_host::ensure_page_enabled(&self.session, &sid).await;
        Ok(id)
    }

    /// 终结引擎，只对 [`EngineSource::Spawned`] 生效；附着来源原样保留
    /// （铁律：绝不关用户的浏览器）。
    ///
    /// 自起实例先 `Browser.close` 优雅退（走守卫旁路），5 秒内 `try_wait`
    /// 轮询等退，不退兜底杀进程树。
    ///
    /// # Errors
    ///
    /// spawn 来源终结失败（杀不掉）。
    pub async fn shutdown(&self) -> Result<()> {
        let mut inner = self.inner.lock().await;
        let EngineSource::Spawned { pid, .. } = &inner.source else {
            return Ok(());
        };
        let pid = *pid;
        let _ = self.session.graceful_close_browser().await;
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut exited = false;
        if let Some(child) = inner.child.as_mut() {
            while Instant::now() < deadline {
                match child.try_wait() {
                    Ok(Some(_)) => {
                        exited = true;
                        break;
                    }
                    Ok(None) => tokio::time::sleep(Duration::from_millis(200)).await,
                    Err(_) => break,
                }
            }
        }
        if !exited {
            cdp_spawn::terminate_pid(pid)?;
        }
        // 隔离态 profile 随引擎退场删除（#25.3 拆出），不留站点痕迹
        if let Some(dir) = inner.isolated_dir.take() {
            let res =
                tokio::task::spawn_blocking(move || std::fs::remove_dir_all(&dir).map(|_| dir))
                    .await;
            // 失败留痕（评审 G）：Windows 刚退的 chrome 常留锁文件，静默积累
            // 会破「不留站点痕迹」承诺；残留目录与后续 isolated 不冲突（时间戳）
            if let Ok(Err(e)) = res {
                eprintln!("[browse] 隔离目录清理失败（残留待手清，评审 G）：{e}");
            }
        }
        inner.source = EngineSource::NotConnected;
        inner.child = None;
        Ok(())
    }

    /// 产出 `/health` 面的状态 JSON（实例名、引擎来源、连接与活动路由）。
    /// #60 操作时刻上下文快照：provenance（#59 语义面）加 fingerprint
    /// （比对用身份面）一次取出，eval 信封的 engineContext 键同源。
    pub async fn context_snapshot(&self, daemon: &crate::server::DaemonDesc) -> serde_json::Value {
        let source = self.source().await;
        let isolated = self.inner.lock().await.isolated_dir.is_some();
        json!({
            "provenance": provenance(&source, isolated, daemon.os, &daemon.hostname),
            "fingerprint": fingerprint_of(&source, isolated),
        })
    }

    /// /health 与 status 的回执体（含 #59 的 daemon 自描述与
    /// engineProvenance 两键；调用方透传 daemon 描述与 uptime）。
    pub async fn health_json(
        &self,
        uptime: Duration,
        daemon: &crate::server::DaemonDesc,
    ) -> serde_json::Value {
        let source = self.source().await;
        let mut v = json!({
            "ok": true,
            "name": crate::paths::instance_name().unwrap_or_else(|| "default".into()),
            "uptime": uptime.as_secs(),
            "connected": self.session.is_connected(),
            "activeTargetId": self.session.active_target().await,
            "activeSessionId": self.session.get_active_session().await,
            // #59：daemon 自描述（CLI 跨宿主比对）与引擎来源语义面
            "daemon": daemon.to_json(),
        });
        v["engine"] = serde_json::to_value(&source).unwrap_or(serde_json::Value::Null);
        let isolated = self.inner.lock().await.isolated_dir.is_some();
        v["engineProvenance"] = provenance(&source, isolated, daemon.os, &daemon.hostname);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #59 health_json 装配面：daemon 自描述四字段在位、未连接态
    /// provenance origin=none（键常在，向后兼容加法不改旧键）。
    #[tokio::test]
    async fn health_json_carries_daemon_self_desc() {
        let session = cdp::Session::new();
        let engine = Engine::new(session);
        let desc = crate::server::DaemonDesc::capture();
        let v = engine.health_json(Duration::ZERO, &desc).await;
        assert!(v["daemon"]["pid"].as_u64().is_some_and(|p| p > 0));
        assert!(!v["daemon"]["os"].as_str().unwrap_or("").is_empty());
        assert!(!v["daemon"]["hostname"].as_str().unwrap_or("").is_empty());
        assert_eq!(v["engineProvenance"]["origin"], "none");
        // 旧键全在（向后兼容判据）
        for k in ["ok", "name", "uptime", "connected", "engine"] {
            assert!(v.get(k).is_some(), "旧键 {k} 不得被移除");
        }
    }

    /// #60 指纹比对：无变化空 vec；pid 换新、有头无头翻转、形态翻转
    /// （持久对隔离）、来源变化（附着对 spawn 键集不同按有无出句）。
    #[test]
    fn fingerprint_diff_shapes() {
        let fp = |origin: &str, pid: Option<u32>, headless: bool, form: &str| {
            serde_json::json!({
                "origin": origin, "pid": pid, "headless": headless,
                "channel": "port", "profileForm": form,
            })
        };
        // 无变化
        let a = fp("managed-spawn", Some(7), true, "persistent");
        assert!(fingerprint_changes(&a, &a).is_empty(), "同指纹零变化");
        // pid 换新（引擎重启）
        let b = fp("managed-spawn", Some(9), true, "persistent");
        assert_eq!(
            fingerprint_changes(&a, &b),
            vec!["引擎换新：7 -> 9".to_string()]
        );
        // 有头无头翻转
        let c = fp("managed-spawn", Some(7), false, "persistent");
        assert_eq!(
            fingerprint_changes(&a, &c),
            vec!["有头无头翻转：无头 -> 有头".to_string()]
        );
        // 形态翻转（持久对隔离，origin 同随）：G1 新语义只出来源句
        let d = fp("isolated-spawn", Some(7), true, "isolated");
        assert_eq!(
            fingerprint_changes(&a, &d),
            vec!["来源变化：managed-spawn -> isolated-spawn".to_string()]
        );
        // 来源切换（附着对 spawn：键集不同只出来源句，无假翻转噪声）
        let att = serde_json::json!({ "origin": "attached", "attachedHost": "127.0.0.1" });
        assert_eq!(
            fingerprint_changes(&a, &att),
            vec!["来源变化：managed-spawn -> attached".to_string()]
        );
    }

    /// #59 provenance 四态：托管 spawn（权威宿主）、隔离 spawn（origin
    /// 区分）、附着 loopback（降级标注不冒充）、附着远端 host（字面）。
    #[test]
    fn provenance_four_shapes() {
        let spawned = EngineSource::Spawned {
            ws_url: Some("ws://127.0.0.1:1/x".into()),
            pid: 7,
            profile_dir: "/p".into(),
            chrome: "/c".into(),
            headless: false,
            channel: "port",
            spawned_at: 42_000,
        };
        let p = provenance(&spawned, false, "linux", "wsl-a");
        assert_eq!(p["origin"], "managed-spawn");
        assert_eq!(p["hostContext"], "linux/wsl-a");
        assert_eq!(p["spawnedBy"], "daemon");
        assert_eq!(p["spawnedAt"], 42_000);

        let p = provenance(&spawned, true, "linux", "wsl-a");
        assert_eq!(p["origin"], "isolated-spawn");

        let att = EngineSource::Attached {
            ws_url: "ws://127.0.0.1:9222/devtools/browser/x".into(),
        };
        let p = provenance(&att, false, "windows", "host-pc");
        assert_eq!(p["origin"], "attached");
        assert!(p["hostContext"].as_str().unwrap().contains("不可辨"));

        let att_remote = EngineSource::Attached {
            ws_url: "ws://lan-ubuntu:9222/devtools/browser/x".into(),
        };
        let p = provenance(&att_remote, false, "linux", "wsl-a");
        assert_eq!(p["hostContext"], "lan-ubuntu");

        // IPv6 loopback（评审 F2）：含括号整段且走降级标注
        let att_v6 = EngineSource::Attached {
            ws_url: "ws://[::1]:9222/devtools/browser/x".into(),
        };
        let p = provenance(&att_v6, false, "linux", "wsl-a");
        assert_eq!(p["hostContext"], "[::1]（loopback：本机或隧道转发不可辨）");

        let p = provenance(&EngineSource::NotConnected, false, "linux", "wsl-a");
        assert_eq!(p["origin"], "none");
        assert!(p["spawnedAt"].is_null());
    }

    /// 代理与引擎附加旗标派生（#25.5/#48）：双开与单开形、附加旗标
    /// 原样透传、非 Auto 面为空。
    #[test]
    fn spawn_extra_args_shape() {
        let spec = EngineSpec::Auto {
            chrome: None,
            headless: true,
            pipe: false,
            profile: None,
            proxy: Some("http://127.0.0.1:7890".into()),
            proxy_bypass: Some("localhost,*.corp".into()),
            isolated: false,
            engine_args: vec!["--enable-features=CleanChromeBrowseDomain".into()],
        };
        assert_eq!(
            spec.spawn_extra_args(),
            vec![
                "--proxy-server=http://127.0.0.1:7890".to_string(),
                "--proxy-bypass-list=localhost,*.corp".to_string(),
                "--enable-features=CleanChromeBrowseDomain".to_string(),
            ]
        );
        let none = EngineSpec::Auto {
            chrome: None,
            headless: false,
            pipe: false,
            profile: None,
            proxy: None,
            proxy_bypass: None,
            isolated: true,
            engine_args: Vec::new(),
        };
        assert!(none.spawn_extra_args().is_empty());
        assert!(
            EngineSpec::Attach {
                ws_url: "ws://x".into()
            }
            .spawn_extra_args()
            .is_empty()
        );
    }
}
