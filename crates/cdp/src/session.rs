//! 一条 browser-level WebSocket + flatten attach + sessionId 路由。
//!
//! 移植自 browser-harness-rs `src/session.rs`，差异：
//! 事件缓冲有上限（防长跑泄漏）；[`Session::call`] 内置安全守卫
//! （拦 `Browser.close` 等破坏性方法、`Target.closeTarget` 只放行自建 tab）。

use anyhow::{Context, Result, anyhow};
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::time::Duration;
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::time::timeout;
use tokio_tungstenite::{connect_async, tungstenite::Message};

/// 事件环形缓冲上限。超过后丢最老的事件，防 daemon 长跑内存泄漏。
const EVENT_BUFFER_CAP: usize = 1000;

/// 单条 CDP 调用的超时（秒）。
const CALL_TIMEOUT_SECS: u64 = 30;

/// 走 browser 端点、不带 sessionId 的域前缀。
const BROWSER_METHODS: &[&str] = &[
    "Browser.",
    "Target.",
    "Storage.",
    "SystemInfo.",
    "Tethering.",
    "Tracing.",
    "Extensions.",
];

/// 一个可附着的 page target（已滤 `chrome://`、`devtools://`）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct PageTarget {
    /// CDP targetId，用于 [`Session::use_target`]。
    pub target_id: String,
    /// 页面标题。
    pub title: String,
    /// 页面 URL。
    pub url: String,
    /// target 类型（这里恒为 `page`）。
    #[serde(rename = "type")]
    pub type_: String,
}

/// 连接线索三选一：`wsUrl` / `port` / `profileDir`。
#[derive(Debug, Clone, Default)]
pub struct ConnectOptions {
    /// 直接给 `ws://127.0.0.1:9222/devtools/browser/<uuid>`（或 http 端点，会取 `/json/version`）。
    pub ws_url: Option<String>,
    /// 调试端口，默认按 `http://127.0.0.1:<port>/json/version` 解析。
    pub port: Option<u16>,
    /// Chrome user-data 目录，读其中的 `DevToolsActivePort` 文件。
    pub profile_dir: Option<String>,
}

/// 常驻 CDP 会话。clone `Arc<Self>` 共享同一条连接。
pub struct Session {
    outgoing: Mutex<Option<mpsc::UnboundedSender<Value>>>,
    pending: Arc<Mutex<HashMap<i64, oneshot::Sender<Value>>>>,
    events: Arc<Mutex<VecDeque<Value>>>,
    next_id: AtomicI64,
    session_id: Mutex<Option<String>>,
    target_id: Mutex<Option<String>>,
    own_targets: Arc<Mutex<HashSet<String>>>,
    connected: AtomicBool,
}

impl Session {
    /// 建一个未连接的会话。连接用 [`Session::connect_opts`] 或 [`Session::connect`]。
    ///
    /// # Examples
    ///
    /// ```
    /// let s = cdp::Session::new();
    /// assert!(!s.is_connected());
    /// ```
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            outgoing: Mutex::new(None),
            pending: Arc::new(Mutex::new(HashMap::new())),
            events: Arc::new(Mutex::new(VecDeque::new())),
            next_id: AtomicI64::new(1),
            session_id: Mutex::new(None),
            target_id: Mutex::new(None),
            own_targets: Arc::new(Mutex::new(HashSet::new())),
            connected: AtomicBool::new(false),
        })
    }

    /// 是否仍连着（WS 读循环存活期间为 true）。
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    /// 按线索连接。已连接时重复调用会再开一条连接（先 [`Session::connect`] 前自查）。
    ///
    /// # Errors
    ///
    /// - WS 握手失败（浏览器没开 / 端口不对）。
    /// - `profileDir` 下 5 秒内读不到 `DevToolsActivePort`。
    pub async fn connect_opts(self: &Arc<Self>, opts: ConnectOptions) -> Result<()> {
        let ws = crate::discovery::resolve_ws_url(&opts).await?;
        self.open_ws(&ws).await
    }

    /// 按字符串连接：`ws://`/`wss://` 直用；`9222` 或 `http://127.0.0.1:9222` 解析端口；
    /// 其余当 http 端点取 `/json/version`。
    ///
    /// # Errors
    ///
    /// 同 [`Session::connect_opts`]。
    pub async fn connect(self: &Arc<Self>, url: &str) -> Result<()> {
        let opts = if url.starts_with("ws://") || url.starts_with("wss://") {
            ConnectOptions {
                ws_url: Some(url.to_string()),
                ..Default::default()
            }
        } else if let Some(port) = crate::parse_port(url) {
            ConnectOptions {
                port: Some(port),
                ..Default::default()
            }
        } else {
            ConnectOptions {
                ws_url: Some(crate::discovery::http_version_ws_url(url).await?),
                ..Default::default()
            }
        };
        self.connect_opts(opts).await
    }

    /// 当前活动 tab 的 targetId（[`Session::use_target`] 设置）。
    pub async fn active_target(&self) -> Option<String> {
        self.target_id.lock().await.clone()
    }

    async fn open_ws(self: &Arc<Self>, ws_url: &str) -> Result<()> {
        let (ws, _) = connect_async(ws_url)
            .await
            .with_context(|| format!("ws connect {ws_url}"))?;
        let (mut write, mut read) = ws.split();
        let (tx, mut rx) = mpsc::unbounded_channel::<Value>();

        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if write.send(Message::Text(msg.to_string())).await.is_err() {
                    break;
                }
            }
        });

        let pending_r = self.pending.clone();
        let events_r = self.events.clone();
        let connected = Arc::new(AtomicBool::new(true));
        let flag = connected.clone();
        tokio::spawn(async move {
            while let Some(Ok(Message::Text(t))) = read.next().await {
                if let Ok(v) = serde_json::from_str::<Value>(&t) {
                    route(v, &pending_r, &events_r).await;
                }
            }
            flag.store(false, Ordering::Relaxed);
        });

        *self.outgoing.lock().await = Some(tx);
        self.connected.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// 接上一对 CDP 管道（clean-chrome 管道态，S005 契约）。
    ///
    /// 协议是 ASCIIZ：NUL 分隔的 JSON-RPC，与 WS Text 帧同构，
    /// 路由/守卫/事件缓冲与 WS 通道完全共用。断线（读端 EOF）即置
    /// `connected=false`；clean-chrome 侧 pipe 断开会自行关浏览器，
    /// 生命周期与启动器绑定。
    ///
    /// # Errors
    ///
    /// 仅在内部通道装配失败时出错（正常路径无网络 IO）。
    ///
    /// # Panics
    ///
    /// 管道泵里的锁只在「持锁线程先前已 panic」的中毒锁上 panic（实际不可达）。
    pub async fn connect_pipes<R, W>(self: &Arc<Self>, read: R, write: W) -> Result<()>
    where
        R: std::io::Read + Send + 'static,
        W: std::io::Write + Send + 'static,
    {
        let (tx, mut rx) = mpsc::unbounded_channel::<Value>();
        let write = Arc::new(std::sync::Mutex::new(write));
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                let mut buf = msg.to_string().into_bytes();
                buf.push(b'\0');
                let w = write.clone();
                let res = tokio::task::spawn_blocking(move || {
                    w.lock().expect("pipe write lock").write_all(&buf)
                })
                .await;
                if !matches!(res, Ok(Ok(()))) {
                    break;
                }
            }
        });

        let pending_r = self.pending.clone();
        let events_r = self.events.clone();
        let connected = Arc::new(AtomicBool::new(true));
        let flag = connected.clone();
        let read = Arc::new(std::sync::Mutex::new(read));
        tokio::spawn(async move {
            let mut carry: Vec<u8> = Vec::new();
            loop {
                let r = read.clone();
                let chunk = tokio::task::spawn_blocking(move || {
                    let mut b = [0u8; 8192];
                    let n = r.lock().expect("pipe read lock").read(&mut b)?;
                    Ok::<_, std::io::Error>(b[..n].to_vec())
                })
                .await
                .unwrap_or_else(|_| Err(std::io::Error::other("join")));
                let chunk = match chunk {
                    Ok(c) if !c.is_empty() => c,
                    _ => break, // EOF / 管道断：浏览器侧没了
                };
                carry.extend_from_slice(&chunk);
                while let Some(pos) = carry.iter().position(|&c| c == 0) {
                    let frame: Vec<u8> = carry.drain(..=pos).collect();
                    if let Ok(v) = serde_json::from_slice(&frame[..frame.len() - 1]) {
                        route(v, &pending_r, &events_r).await;
                    }
                }
            }
            flag.store(false, Ordering::Relaxed);
        });

        *self.outgoing.lock().await = Some(tx);
        self.connected.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// 发一条 CDP 命令。非 browser 域自动带活动 tab 的 `sessionId`。
    ///
    /// 安全守卫（取自 bh 硬边界契约，程序级强制）：
    /// `Browser.close` / `Browser.setWindowBounds` 一律拒绝；
    /// `Target.closeTarget` 只放行本会话自建 tab；`Target.createTarget`
    /// 的产物自动登记为自建 tab。引擎优雅退出走 [`Session::graceful_close_browser`]。
    ///
    /// # Errors
    ///
    /// - 未连接（先 `session.connect(...)`）。
    /// - 被 [`Session::call`] 的守卫拦截（错误信息附下一步指令）。
    /// - CDP 返回 error、或 30 秒超时。
    pub async fn call(&self, method: &str, params: Value) -> Result<Value> {
        self.guard(method, &params).await?;
        let v = self.send(method, params).await?;
        if method == "Target.createTarget"
            && let Some(id) = v.get("targetId").and_then(Value::as_str)
        {
            self.own_targets.lock().await.insert(id.to_string());
        }
        Ok(v)
    }

    async fn guard(&self, method: &str, params: &Value) -> Result<()> {
        match method {
            "Browser.close" => Err(anyhow!(
                "browse: 守卫拦截 Browser.close（绝不关闭附着的浏览器）；\
                 自起引擎用 browse down 退出"
            )),
            "Browser.setWindowBounds" => Err(anyhow!(
                "browse: 守卫拦截 Browser.setWindowBounds（不动用户窗口）"
            )),
            "Target.closeTarget" => {
                let id = params.get("targetId").and_then(Value::as_str).unwrap_or("");
                if self.own_targets.lock().await.contains(id) {
                    Ok(())
                } else {
                    Err(anyhow!(
                        "browse: 守卫拦截 Target.closeTarget：{id} 不是本会话自建 tab，\
                         只关自己开的 tab（守卫在 Session::call 层，绕不过）"
                    ))
                }
            }
            _ => Ok(()),
        }
    }

    /// 仅供引擎自己对 spawn 出来的浏览器做优雅退出：直发 `Browser.close`，绕过守卫。
    /// 附着来源绝不允许调用（调用方自查来源）。
    ///
    /// # Errors
    ///
    /// 未连接或 CDP 报错。
    pub async fn graceful_close_browser(&self) -> Result<Value> {
        self.send("Browser.close", json!({})).await
    }

    async fn send(&self, method: &str, params: Value) -> Result<Value> {
        if !self.is_connected() {
            return Err(anyhow!("Not connected. Call session.connect(...) first."));
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut msg = json!({ "id": id, "method": method, "params": params });
        if !is_browser_method(method)
            && let Some(sid) = self.session_id.lock().await.clone()
        {
            msg["sessionId"] = json!(sid);
        }
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        self.outgoing
            .lock()
            .await
            .as_ref()
            .ok_or_else(|| anyhow!("CDP socket closed"))?
            .send(msg)
            .map_err(|_| anyhow!("CDP socket closed"))?;

        let resp = timeout(Duration::from_secs(CALL_TIMEOUT_SECS), rx)
            .await
            .context("cdp timeout")?
            .context("cdp dropped")?;
        if let Some(err) = resp.get("error") {
            return Err(anyhow!("CDP {method}: {err}"));
        }
        Ok(resp.get("result").cloned().unwrap_or(json!({})))
    }

    /// 附着某 tab 并设为活动路由（`Target.attachToTarget{flatten:true}`）。
    ///
    /// # Errors
    ///
    /// CDP attach 失败（target 已消失等）。
    pub async fn use_target(&self, target_id: &str) -> Result<String> {
        let r = self
            .send(
                "Target.attachToTarget",
                json!({ "targetId": target_id, "flatten": true }),
            )
            .await?;
        let sid = r
            .get("sessionId")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("no sessionId"))?
            .to_string();
        *self.session_id.lock().await = Some(sid.clone());
        *self.target_id.lock().await = Some(target_id.to_string());
        Ok(sid)
    }

    /// 覆写活动 sessionId（高级用法；一般走 [`Session::use_target`]）。
    pub async fn set_active_session(&self, session_id: Option<String>) {
        *self.session_id.lock().await = session_id;
    }

    /// 当前活动 sessionId（无则 `None`）。
    pub async fn get_active_session(&self) -> Option<String> {
        self.session_id.lock().await.clone()
    }

    /// 列可附着的 page targets（滤 `chrome://`、`devtools://`，防 attach 到
    /// 1px omnibox 弹窗）。返回顺序不保证等于标签栏可见顺序。
    ///
    /// # Errors
    ///
    /// 未连接或 `Target.getTargets` 失败。
    pub async fn list_page_targets(&self) -> Result<Vec<PageTarget>> {
        let r = self.send("Target.getTargets", json!({})).await?;
        let infos = r
            .get("targetInfos")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        Ok(infos
            .into_iter()
            .filter_map(|t| {
                let type_ = t.get("type")?.as_str()?.to_string();
                let url = t.get("url")?.as_str()?.to_string();
                if type_ != "page" {
                    return None;
                }
                if url.starts_with("chrome://") || url.starts_with("devtools://") {
                    return None;
                }
                Some(PageTarget {
                    target_id: t.get("targetId")?.as_str()?.to_string(),
                    title: t
                        .get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    url,
                    type_,
                })
            })
            .collect())
    }

    /// 新建 tab（自动登记为自建 tab，可被 `Target.closeTarget` 关闭）。
    ///
    /// # Errors
    ///
    /// 未连接或 `Target.createTarget` 失败。
    pub async fn create_target(&self, url: &str) -> Result<String> {
        let r = self
            .call("Target.createTarget", json!({ "url": url }))
            .await?;
        r.get("targetId")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| anyhow!("createTarget: no targetId"))
    }

    /// 从环形缓冲里找第一个 `method` 事件（取出即移除）。超时报错。
    ///
    /// # Errors
    ///
    /// `wait_ms` 内没等到该事件。
    pub async fn wait_for(&self, method: &str, wait_ms: u64) -> Result<Value> {
        let deadline = tokio::time::Instant::now() + Duration::from_millis(wait_ms);
        loop {
            {
                let mut evs = self.events.lock().await;
                if let Some(i) = evs
                    .iter()
                    .position(|e| e.get("method").and_then(|m| m.as_str()) == Some(method))
                {
                    return Ok(evs.remove(i).unwrap_or(Value::Null));
                }
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(anyhow!("Timeout waiting for {method}"));
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }
}

/// 把一条入站 JSON-RPC 消息路由到 pending 应答或事件缓冲（WS/管道共用）。
async fn route(
    v: Value,
    pending: &Arc<Mutex<HashMap<i64, oneshot::Sender<Value>>>>,
    events: &Arc<Mutex<VecDeque<Value>>>,
) {
    if let Some(id) = v.get("id").and_then(|x| x.as_i64()) {
        if let Some(tx) = pending.lock().await.remove(&id) {
            let _ = tx.send(v);
        }
    } else if v.get("method").is_some() {
        let mut evs = events.lock().await;
        if evs.len() >= EVENT_BUFFER_CAP {
            evs.pop_front();
        }
        evs.push_back(v);
    }
}

/// 方法是否属于 browser 端点域（不附 `sessionId`）。
///
/// # Examples
///
/// ```
/// assert!(cdp::session::is_browser_method("Target.getTargets"));
/// assert!(!cdp::session::is_browser_method("Page.navigate"));
/// ```
pub fn is_browser_method(method: &str) -> bool {
    BROWSER_METHODS.iter().any(|p| method.starts_with(p))
}
