//! daemon 的 HTTP API：常驻会话 + 方言求值 + 引擎生命周期。
//!
//! 移植自 browser-harness-rs `src/server.rs`（POST /eval、GET /health、
//! POST /quit 三端点保留），新增 `/engine/up` 与求值前的懒引擎策略：
//!
//! - 求值前未连接且片段里没有 `session.connect` -> 自动 [`crate::Engine::ensure`]；
//!   片段自带 `session.connect` 则不预连（显式意图优先，ADR-0004）。
//! - 求值撞上「Not connected」再兜底 ensure + 重试一次（覆盖先语句后连接的写法）。
//! - 单飞槽：同一时刻只跑一条片段，后来者排队（不拒 429）。
//! - `/eval` 超时默认 300 秒（`BROWSE_EVAL_TIMEOUT`，单位秒）。

use crate::{Engine, EngineSpec, JsHost};
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::net::TcpListener;

/// 描述 POST /eval 的请求体。
#[derive(Debug, Deserialize)]
pub struct EvalRequest {
    /// 方言片段源码（或裸 JS，由 body 形态决定，见 [`serve`]）。
    pub code: String,
    /// 求值前先新开 about:blank tab（`browse --new-tab` 面）。
    #[serde(default)]
    pub new_tab: bool,
}

/// 描述 POST /engine/up 的请求体。
#[derive(Debug, Default, Deserialize)]
pub struct EngineUpRequest {
    /// spawn 时无头。
    #[serde(default)]
    pub headless: bool,
    /// 指定 chrome 可执行文件。
    pub chrome: Option<String>,
    /// 显式 WS URL。
    pub ws: Option<String>,
    /// 显式端口。
    pub port: Option<u16>,
    /// spawn 走 CDP 管道通道（S005，Windows）。
    #[serde(default)]
    pub pipe: bool,
}

/// HTTP daemon 的运行面聚合：宿主、引擎、缺省引擎意图、单飞槽与退出旗标。
pub struct Daemon {
    /// 方言宿主（vars 跨请求持久）。
    pub host: Arc<JsHost>,
    /// 引擎状态机。
    pub engine: Arc<Engine>,
    /// 没有显式意图时的引擎策略（来自 CLI 旗标 / 环境）。
    pub spec: EngineSpec,
    /// 单飞槽：同一时刻一条片段。
    pub eval_lock: tokio::sync::Mutex<()>,
    /// 退出旗标（POST /quit 置位）。
    pub quit: Arc<AtomicBool>,
    /// daemon 启动时刻。
    pub started: Instant,
}

impl Daemon {
    /// 把宿主、引擎与缺省意图组装成运行面。
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # // no_run：JsHost 构造会 spawn watcher 任务，需要 tokio runtime
    /// let session = cdp::Session::new();
    /// let d = browse_core::server::Daemon::new(
    ///     browse_core::JsHost::new(session.clone()),
    ///     browse_core::Engine::new(session),
    ///     browse_core::EngineSpec::from_env(None, false, false),
    /// );
    /// assert!(std::sync::Arc::strong_count(&d) >= 1);
    /// ```
    pub fn new(host: Arc<JsHost>, engine: Arc<Engine>, spec: EngineSpec) -> Arc<Self> {
        Arc::new(Self {
            host,
            engine,
            spec,
            eval_lock: tokio::sync::Mutex::new(()),
            quit: Arc::new(AtomicBool::new(false)),
            started: Instant::now(),
        })
    }
}

fn eval_timeout() -> Duration {
    std::env::var("BROWSE_EVAL_TIMEOUT")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(300))
}

/// 起 HTTP daemon，监听 `bind`，直到 POST /quit。
///
/// # Errors
///
/// 端口绑定失败或 axum serve 出错。
pub async fn serve(daemon: Arc<Daemon>, bind: &str) -> anyhow::Result<()> {
    let state = AppState {
        daemon: daemon.clone(),
    };
    let app = Router::new()
        .route("/eval", post(eval_handler))
        .route("/health", get(health_handler))
        .route("/engine/up", post(engine_up_handler))
        .route("/quit", post(quit_handler))
        .with_state(state);

    let listener = TcpListener::bind(bind).await?;
    eprintln!(
        "{}",
        json!({
            "ok": true,
            "ready": true,
            "bind": bind,
            "message": format!("browse daemon listening on http://{bind}"),
        })
    );
    let quit = daemon.quit.clone();
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            while !quit.load(Ordering::Relaxed) {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await?;
    daemon.engine.shutdown().await?;
    Ok(())
}

#[derive(Clone)]
struct AppState {
    daemon: Arc<Daemon>,
}

async fn eval_handler(
    State(st): State<AppState>,
    Json(req): Json<EvalRequest>,
) -> impl IntoResponse {
    let _slot = st.daemon.eval_lock.lock().await;
    let d = &st.daemon;
    if req.new_tab
        && let Err(e) = d.engine.new_tab().await
    {
        return err_response(e);
    }
    // 懒引擎：未连接且片段没打算自己连，才预连
    if !d.host.session().is_connected()
        && !req.code.contains("connect")
        && let Err(e) = d.engine.ensure(&d.spec).await
    {
        return err_response(e);
    }
    let outcome = tokio::time::timeout(eval_timeout(), d.host.eval_snippet(&req.code)).await;
    let value = match outcome {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            // 撞上「还没连」的片段：兜底拉引擎重试一次
            if format!("{e:#}").contains("Not connected") {
                match d.engine.ensure(&d.spec).await {
                    Ok(_) => match d.host.eval_snippet(&req.code).await {
                        Ok(v) => v,
                        Err(e2) => return err_response(e2),
                    },
                    Err(e2) => return err_response(e2),
                }
            } else {
                return err_response(e);
            }
        }
        Err(_) => {
            return err_response(anyhow::anyhow!(
                "eval 超时（{} 秒；BROWSE_EVAL_TIMEOUT 可调）",
                eval_timeout().as_secs()
            ));
        }
    };
    (
        axum::http::StatusCode::OK,
        Json(json!({ "ok": true, "value": value })),
    )
}

fn err_response(e: anyhow::Error) -> (axum::http::StatusCode, Json<Value>) {
    // 前缀在此统一加：错误串形态是「browse: <下一步指令>」进 stderr
    (
        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "ok": false, "error": format!("browse: {e:#}") })),
    )
}

async fn health_handler(State(st): State<AppState>) -> impl IntoResponse {
    Json(
        st.daemon
            .engine
            .health_json(st.daemon.started.elapsed())
            .await,
    )
}

async fn engine_up_handler(
    State(st): State<AppState>,
    Json(req): Json<EngineUpRequest>,
) -> impl IntoResponse {
    let spec = if let Some(ws) = req.ws {
        EngineSpec::Attach { ws_url: ws }
    } else if let Some(p) = req.port {
        EngineSpec::Port(p)
    } else {
        EngineSpec::Auto {
            chrome: req.chrome.map(Into::into),
            headless: req.headless,
            pipe: req.pipe,
        }
    };
    match st.daemon.engine.ensure(&spec).await {
        Ok(_) => (
            axum::http::StatusCode::OK,
            Json(
                st.daemon
                    .engine
                    .health_json(st.daemon.started.elapsed())
                    .await,
            ),
        ),
        Err(e) => err_response(e),
    }
}

async fn quit_handler(State(st): State<AppState>) -> impl IntoResponse {
    st.daemon.quit.store(true, Ordering::Relaxed);
    Json(json!({ "ok": true }))
}
