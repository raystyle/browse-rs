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
    /// 方言片段源码；`js` 为真时是全量 JS（不经方言解析器，#22 旁路）。
    pub code: String,
    /// 求值前先新开 about:blank tab（`browse --new-tab` 面）。
    #[serde(default)]
    pub new_tab: bool,
    /// 全量 JS 形态（`browse --js` 面，#22）：走 [`JsHost::eval_js`]。
    #[serde(default)]
    pub js: bool,
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
    /// spawn 引擎的自定义 profile（user-data-dir；缺省用固定 engine-profile）。
    pub profile: Option<String>,
    /// 引擎代理（`--proxy-server`，#25.5）。
    pub proxy: Option<String>,
    /// 代理旁路清单（`--proxy-bypass-list`，#25.5）。
    pub proxy_bypass: Option<String>,
    /// 隔离态 profile：引擎退出即删（#25.3 拆出）。
    #[serde(default)]
    pub isolated: bool,
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
    /// 最近活动时刻（#25.1 idle-timeout 判据）：eval/up 请求刷新。
    pub last_activity: std::sync::Mutex<Instant>,
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
            last_activity: std::sync::Mutex::new(Instant::now()),
        })
    }
}

/// 引擎闲置回收窗（#25.1）：`BROWSE_IDLE_TIMEOUT` 毫秒，缺省 1 小时，
/// `0` 关闭。到期引擎退役（daemon 不退），下次求值懒拉起。
fn idle_timeout() -> Duration {
    std::env::var("BROWSE_IDLE_TIMEOUT")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(Duration::from_millis(3_600_000))
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
        // 只读看板（#54）：GET / 出 HTML 单页，/dashboard/sse 推事件流；
        // 看板自身不是工作 tab，禁止被附着驾驶（无写路由佐证）
        .route("/", get(dashboard_handler))
        .route("/dashboard/sse", get(dashboard_sse_handler))
        .with_state(state);

    // 闲置回收机（#25.1）：到期退引擎不退 daemon，下次求值自动拉起
    {
        let daemon = daemon.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                let timeout = idle_timeout();
                if timeout.is_zero() {
                    continue;
                }
                let idle_for = daemon
                    .last_activity
                    .lock()
                    .map(|t| t.elapsed())
                    .unwrap_or(Duration::ZERO);
                if idle_for < timeout || !daemon.host.session().is_connected() {
                    continue;
                }
                // 与求值共用单飞槽（评审 F1）：不撕断在跑的求值，也防
                // 「引擎退役 -> Not connected 兜底 -> 整段重放」的副作用重放；
                // 拿到槽后二次确认（等槽期间可能刚来了求值）
                let _slot = daemon.eval_lock.lock().await;
                let still_idle = daemon
                    .last_activity
                    .lock()
                    .map(|t| t.elapsed())
                    .unwrap_or(Duration::ZERO);
                if still_idle < timeout || !daemon.host.session().is_connected() {
                    continue;
                }
                eprintln!(
                    "{}",
                    json!({
                        "idleRetire": true,
                        "idleMs": still_idle.as_millis() as u64,
                        "note": "引擎闲置退役，下次求值自动拉起（BROWSE_IDLE_TIMEOUT 可调，0 关闭）",
                    })
                );
                let _ = daemon.engine.shutdown().await;
            }
        });
    }

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

/// /eval 的语言分流（#22）：`js` 旗标走全量 JS 旁路（[`JsHost::eval_js`]，
/// 不经方言解析器），缺省走方言片段。
async fn run_eval_code(host: &JsHost, code: &str, js: bool) -> anyhow::Result<Value> {
    if js {
        host.eval_js(code).await
    } else {
        host.eval_snippet(code).await
    }
}

async fn eval_handler(
    State(st): State<AppState>,
    Json(req): Json<EvalRequest>,
) -> impl IntoResponse {
    let _ = st
        .daemon
        .last_activity
        .lock()
        .map(|mut t| *t = Instant::now());
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
    // 分流（#22）：js 旗标走全量 JS 旁路，缺省走方言
    let outcome =
        tokio::time::timeout(eval_timeout(), run_eval_code(&d.host, &req.code, req.js)).await;
    let value = match outcome {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            // 撞上「还没连」的片段：兜底拉引擎重试一次
            if format!("{e:#}").contains("Not connected") {
                match d.engine.ensure(&d.spec).await {
                    Ok(_) => match run_eval_code(&d.host, &req.code, req.js).await {
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
    // 前缀在此统一加：错误串形态是「browse: <下一步指令>」进 stderr。
    // 错误链内已有前缀的（cdp 守卫、引擎面文案）不再叠加，防「browse: browse:」。
    // 错误链过密钥面具再上前缀（#25.4 评审 G4）：守卫文案嵌 id/url、CDP
    // 错误嵌参数，agent 拿 secrets.X 当 token/URL 片段是自然用法
    let msg = crate::js_host::mask_secrets_str(&format!("{e:#}"));
    let msg = if msg.starts_with("browse:") {
        msg
    } else {
        format!("browse: {msg}")
    };
    (
        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "ok": false, "error": msg })),
    )
}

/// 只读看板（#54）：单文件 HTML（无外部资源），1 秒轮询 /health 渲染
/// 实例概览与活动 tab。SSE 通道见 [`dashboard_sse_handler`]。
async fn dashboard_handler() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
        axum::response::Html(DASHBOARD_HTML),
    )
}

/// 看板 SSE（#54）：每 2 秒推一帧 health 快照（instance/uptime/engine/
/// connected/activeTarget/title/url）。axum 0.7 的 SSE 走 Body::from_stream。
async fn dashboard_sse_handler(State(st): State<AppState>) -> impl IntoResponse {
    use futures::stream::StreamExt;
    let daemon = st.daemon.clone();
    let stream = futures::stream::unfold(daemon, |daemon| async move {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let health = daemon.engine.health_json(daemon.started.elapsed()).await;
        let mut ev = String::from("data: ");
        ev.push_str(&health.to_string());
        ev.push_str(
            "

",
        );
        Some((Ok::<_, std::io::Error>(axum::body::Bytes::from(ev)), daemon))
    })
    .map(|r: Result<axum::body::Bytes, std::io::Error>| r);
    let body = axum::body::Body::from_stream(stream);
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/event-stream; charset=utf-8",
        )],
        body,
    )
}

/// 看板 HTML（#54）：inline，无外部依赖；SSE 断线自动重连（EventSource 原生）。
const DASHBOARD_HTML: &str = r#"<!doctype html>
<html lang="zh"><head><meta charset="utf-8">
<title>browse dashboard</title>
<style>
body{font:14px/1.5 system-ui,sans-serif;margin:2rem;max-width:60rem}
table{border-collapse:collapse;margin-top:1rem}
td,th{border:1px solid #ccc;padding:.3rem .7rem;text-align:left}
th{background:#f5f5f5}
.badge{display:inline-block;padding:.1rem .5rem;border-radius:1rem;font-size:.85rem}
.on{background:#d4f7d4}.off{background:#f7d4d4}
#evt{white-space:pre;font:12px/1.4 monospace;background:#fafafa;padding:1rem;margin-top:1rem;overflow:auto;max-height:16rem}
</style></head><body>
<h1>browse dashboard</h1>
<p>只读看板（多实例总览：每实例一个 daemon，各开各的看板；端口见 browse status）。本页不是工作 tab，无任何写操作。</p>
<table id="t"><tr><th>项</th><th>值</th></tr></table>
<div id="evt">（事件流）</div>
<script>
function row(k,v){return '<tr><td>'+k+'</td><td>'+v+'</td></tr>'}
function render(h){
  const eng = h.engine && h.engine.Spawned ? 'spawned pid '+h.engine.Spawned.pid
    : h.engine && h.engine.Attached ? 'attached' : 'not connected';
  const conn = '<span class="badge '+(h.connected?'on':'off')+'">'+(h.connected?'connected':'offline')+'</span>';
  document.getElementById('t').innerHTML =
    row('instance', h.name||'default') + row('uptime(s)', Math.round(h.uptime/1000)) +
    row('engine', eng) + row('connection', conn) +
    row('activeTarget', h.activeTargetId||'-') + row('activeSession', h.activeSessionId||'-');
}
const es = new EventSource('/dashboard/sse');
es.onmessage = e => { const h = JSON.parse(e.data); render(h);
  const d = document.getElementById('evt');
  d.textContent = new Date().toLocaleTimeString()+'  '+JSON.stringify(h)+'
'+d.textContent;
};
es.onerror = () => { document.getElementById('evt').textContent = 'SSE 断线，自动重连中…
'+document.getElementById('evt').textContent; };
</script></body></html>"#;

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
    let _ = st
        .daemon
        .last_activity
        .lock()
        .map(|mut t| *t = Instant::now());
    let spec = if let Some(ws) = req.ws {
        EngineSpec::Attach { ws_url: ws }
    } else if let Some(p) = req.port {
        EngineSpec::Port(p)
    } else {
        // 缺字段回落 from_env（评审 F2）：消除「同一 BROWSE_PROXY/
        // BROWSE_PROFILE 一会生效一会不生效」的旗标与 env 两态。走到本块
        // 时请求必未显式给 ws/port：env 钉了 BROWSE_CDP_WS（Attach）则
        // 钉死意图直通（不 panic，那是手册在册配置，二轮快核雷）
        let env_spec = EngineSpec::from_env(None, false, false);
        let (env_profile, env_proxy, env_bypass, env_ws) = match env_spec {
            EngineSpec::Attach { ws_url } => (None, None, None, Some(ws_url)),
            EngineSpec::Auto {
                profile,
                proxy,
                proxy_bypass,
                ..
            } => (profile, proxy, proxy_bypass, None),
            EngineSpec::Port(_) => (None, None, None, None),
        };
        if let Some(ws_url) = env_ws {
            EngineSpec::Attach { ws_url }
        } else {
            EngineSpec::Auto {
                chrome: req.chrome.map(Into::into),
                headless: req.headless,
                pipe: req.pipe,
                profile: req.profile.map(Into::into).or(env_profile),
                proxy: req.proxy.clone().or(env_proxy),
                proxy_bypass: req.proxy_bypass.clone().or(env_bypass),
                isolated: req.isolated,
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 错误前缀只加一次（#33 快核 G-lite）：cdp 守卫/引擎面文案自带
    /// 「browse: 」时不得叠成「browse: browse:」。
    #[test]
    fn err_response_prefixes_exactly_once() {
        let (_, Json(v)) = err_response(anyhow::anyhow!("browse: 守卫拦截 Browser.close"));
        let msg = v["error"].as_str().unwrap_or_default();
        assert_eq!(msg, "browse: 守卫拦截 Browser.close");
        assert!(!msg.starts_with("browse: browse:"), "{msg}");

        let (_, Json(v)) = err_response(anyhow::anyhow!("CDP Page.navigate: boom"));
        assert_eq!(
            v["error"].as_str().unwrap_or_default(),
            "browse: CDP Page.navigate: boom"
        );
    }

    /// 错误链脱敏调用点锁（#25.4 评审 G4 二轮）：err_response 必须过
    /// [`crate::js_host::mask_secrets_str`]——守卫文案嵌 id/url、CDP 错误
    /// 嵌参数，密钥值不得经错误出口上 stderr；CTA 保留。
    #[tokio::test]
    async fn err_response_masks_secret_values() {
        let _ser = crate::js_host::SECRETS_TEST_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("browse-sec-srv-{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok();
        let f = dir.join("s.env");
        std::fs::write(&f, "TOKEN=sk-live-abc123\n").unwrap();
        crate::js_host::load_secrets(f.to_str().unwrap()).expect("装仓");
        // 复刻守卫错误形态（评审方实弹件：Target.closeTarget 带密钥当 id）
        let (_, Json(v)) = err_response(anyhow::anyhow!(
            "守卫拦截 Target.closeTarget：sk-live-abc123 不是本会话自建 tab；下一步：只关 listPageTargets()"
        ));
        let msg = v["error"].as_str().unwrap_or_default();
        assert!(!msg.contains("sk-live-abc123"), "密钥值应被换: {msg}");
        assert!(msg.contains("***"), "{msg}");
        assert!(msg.contains("下一步"), "CTA 应保留: {msg}");
        // 前缀形态不回归（脱敏不破坏 browse: 头）
        assert!(msg.starts_with("browse: "), "{msg}");
        // 清仓防串测（空文件 = 清空全局）
        std::fs::write(&f, "").unwrap();
        crate::js_host::load_secrets(f.to_str().unwrap()).expect("清仓");
        std::fs::remove_dir_all(&dir).ok();
    }
}
