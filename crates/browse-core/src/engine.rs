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
    /// `--profile` 可约束 spawn 面）。
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
    },
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
    },
}

/// 引擎状态机：确保连接、报告来源、只终结自己 spawn 的。
pub struct Engine {
    session: Arc<Session>,
    inner: Mutex<EngineInner>,
}

struct EngineInner {
    source: EngineSource,
    child: Option<Child>,
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
            } => {
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
                        .spawn_pipes(chrome.clone(), *headless, profile.clone())
                        .await;
                } else {
                    // spawn 分支自己落状态（含 child 句柄）并 attach，提前返回
                    return self.spawn(chrome.clone(), *headless, profile.clone()).await;
                }
            }
        };
        *self.inner.lock().await = EngineInner {
            source: source.clone(),
            child: None,
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
    ) -> Result<EngineSource> {
        let chrome = cdp_spawn::find_chrome(resolve_chrome(chrome).as_deref()).ok_or_else(|| {
            anyhow!(
                "browse: 找不到 chrome；下一步：browse chrome install <版本> <部署目录>（或 --chrome <path> / BROWSE_CHROME 显式指定）"
            )
        })?;
        let profile = profile.unwrap_or_else(crate::paths::engine_profile_dir);
        let child = cdp_spawn::spawn_engine(&chrome, &profile, headless)?;
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
        *self.inner.lock().await = EngineInner {
            source: EngineSource::Spawned {
                ws_url: Some(ws),
                pid,
                profile_dir: profile,
                chrome,
                headless,
                channel: "port",
            },
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
    ) -> Result<EngineSource> {
        let chrome = cdp_spawn::find_chrome(resolve_chrome(chrome).as_deref()).ok_or_else(|| {
            anyhow!(
                "browse: 找不到 chrome；下一步：browse chrome install <版本> <部署目录>（或 --chrome <path> / BROWSE_CHROME 显式指定）"
            )
        })?;
        let profile = profile.unwrap_or_else(crate::paths::engine_profile_dir);
        let engine =
            cdp_spawn::spawn_engine_pipes(&chrome, &profile, headless, cdp_spawn::PipeMode::Pipe)
                .context("管道态 spawn")?;
        let pid = engine.child.id();
        self.session
            .connect_pipes(engine.read, engine.write)
            .await?;
        *self.inner.lock().await = EngineInner {
            source: EngineSource::Spawned {
                ws_url: None,
                pid,
                profile_dir: profile,
                chrome,
                headless,
                channel: "pipe",
            },
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
        inner.source = EngineSource::NotConnected;
        inner.child = None;
        Ok(())
    }

    /// 产出 `/health` 面的状态 JSON（实例名、引擎来源、连接与活动路由）。
    pub async fn health_json(&self, uptime: Duration) -> serde_json::Value {
        let source = self.source().await;
        let mut v = json!({
            "ok": true,
            "name": crate::paths::instance_name().unwrap_or_else(|| "default".into()),
            "uptime": uptime.as_secs(),
            "connected": self.session.is_connected(),
            "activeTargetId": self.session.active_target().await,
            "activeSessionId": self.session.get_active_session().await,
        });
        v["engine"] = serde_json::to_value(&source).unwrap_or(serde_json::Value::Null);
        v
    }
}
