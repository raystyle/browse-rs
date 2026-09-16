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

/// 事件环形缓冲的条数上限；超过后丢最老的事件，防 daemon 长跑内存泄漏。
const EVENT_BUFFER_CAP: usize = 1000;

/// 单条 CDP 调用的超时秒数，到点报 `cdp timeout`。
const CALL_TIMEOUT_SECS: u64 = 30;

/// 这些前缀的域走 browser 端点，调用时不附 `sessionId`。
const BROWSER_METHODS: &[&str] = &[
    "Browser.",
    "Target.",
    "Storage.",
    "SystemInfo.",
    "Tethering.",
    "Tracing.",
    "Extensions.",
];

/// 描述一个可附着的 page target；已滤 `chrome://`、`devtools://` 干扰目标。
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
    /// 是否本会话自建（`Target.createTarget` 产物）：只有 `own` tab 能被
    /// `Target.closeTarget` 关闭；chrome 启动自开的初始页与用户 tab 恒 false。
    pub own: bool,
}

/// 连接线索三选一（`wsUrl` / `port` / `profileDir`），全部可缺省走自动策略。
#[derive(Debug, Clone, Default)]
pub struct ConnectOptions {
    /// 直接给 `ws://127.0.0.1:9222/devtools/browser/<uuid>`（或 http 端点，会取 `/json/version`）。
    pub ws_url: Option<String>,
    /// 调试端口，默认按 `http://127.0.0.1:<port>/json/version` 解析。
    pub port: Option<u16>,
    /// Chrome user-data 目录，读其中的 `DevToolsActivePort` 文件。
    pub profile_dir: Option<String>,
    /// 连接超时毫秒（发现轮询 + WS 握手 + `/json/version` 请求共用）。
    ///
    /// 缺省 5000；要等人工点 Allow 的场景给 30000。对齐官方 harness。
    pub timeout_ms: Option<u64>,
}

/// 缺省连接超时毫秒数，`ConnectOptions::timeout_ms` 未设时生效。
const DEFAULT_CONNECT_TIMEOUT_MS: u64 = 5000;

/// 常驻的 browser 级 CDP 会话；clone `Arc<Self>` 共享同一条连接。
pub struct Session {
    outgoing: Mutex<Option<mpsc::UnboundedSender<Value>>>,
    pending: Arc<Mutex<HashMap<i64, oneshot::Sender<Value>>>>,
    events: Arc<Mutex<VecDeque<Value>>>,
    next_id: AtomicI64,
    session_id: Mutex<Option<String>>,
    target_id: Mutex<Option<String>>,
    own_targets: Arc<Mutex<HashSet<String>>>,
    /// 存活旗：读循环持有克隆，连接断开（WS EOF / 管道 EOF）即翻 false；
    /// [`Session::is_connected`] 与引擎侧的懒 ensure 都看它。必须是
    /// `Arc` 共享给 `'static` 读循环，否则死线只写进局部旗，会话永远
    /// 谎报活着（attach 重附不重建的 2026-09-17 实测缺口即此）。
    connected: Arc<AtomicBool>,
    next_seq: Arc<AtomicI64>,
    /// 当前打开的 `Page.javascriptDialogOpening` 事件（route 截获维护，
    /// Closed 清空）。对话框会挂起 Input/evaluate，消费方要能先看它。
    pending_dialog: Arc<Mutex<Option<Value>>>,
}

impl Session {
    /// 建一个未连接的会话。
    ///
    /// 连接用 [`Session::connect_opts`] 或 [`Session::connect`]。
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
            connected: Arc::new(AtomicBool::new(false)),
            next_seq: Arc::new(AtomicI64::new(1)),
            pending_dialog: Arc::new(Mutex::new(None)),
        })
    }

    /// 连接是否仍活着：WS/管道读循环存活期间为 true。
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    /// 按线索连接，超时由 [`ConnectOptions::timeout_ms`] 控制（缺省 5 秒）。
    ///
    /// 已连接时重复调用会再开一条连接（先 [`Session::connect`] 前自查）。
    ///
    /// # Errors
    ///
    /// - WS 握手失败或超时（浏览器没开 / 端口不对 / 等 Allow 没等到）。
    /// - `profileDir` 下超时内读不到 `DevToolsActivePort`。
    pub async fn connect_opts(self: &Arc<Self>, opts: ConnectOptions) -> Result<()> {
        let ws = crate::discovery::resolve_ws_url(&opts).await?;
        let dur = Duration::from_millis(opts.timeout_ms.unwrap_or(DEFAULT_CONNECT_TIMEOUT_MS));
        timeout(dur, self.open_ws(&ws))
            .await
            .context("ws 握手超时")??;
        Ok(())
    }

    /// 字符串线索的宽容连接：`ws://`/`wss://` 直用，`9222` 或
    /// `http://127.0.0.1:9222` 解析端口，其余当 http 端点取 `/json/version`。
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
                ws_url: Some(
                    crate::discovery::http_version_ws_url(
                        url,
                        Duration::from_millis(DEFAULT_CONNECT_TIMEOUT_MS),
                    )
                    .await?,
                ),
                ..Default::default()
            }
        };
        self.connect_opts(opts).await
    }

    /// 返回事件缓冲当前水位（最新事件的 seq，空缓冲为 0），作
    /// [`Session::peek_events_since`] 的增量起点。
    pub async fn last_seq(&self) -> u64 {
        self.events
            .lock()
            .await
            .back()
            .and_then(|e| e.get("seq").and_then(Value::as_u64))
            .unwrap_or(0)
    }

    /// 返回当前活动 tab 的 targetId（由 [`Session::use_target`] 设置）。
    pub async fn active_target(&self) -> Option<String> {
        self.target_id.lock().await.clone()
    }

    /// 返回当前打开的 JS 对话框事件（`Page.javascriptDialogOpening` 全量），
    /// 无则 `None`。
    ///
    /// 对话框会挂起 Input/evaluate，调用方应先看它再行动。
    pub async fn pending_dialog(&self) -> Option<Value> {
        self.pending_dialog.lock().await.clone()
    }

    /// 手动清掉对话框状态（`dialogAccept/Dismiss` 应答后 Closed 事件
    /// 可能迟到，先清防竞速误报）。
    pub async fn clear_pending_dialog(&self) {
        *self.pending_dialog.lock().await = None;
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
        let seq_r = self.next_seq.clone();
        let dialog_r = self.pending_dialog.clone();
        let flag = self.connected.clone();
        tokio::spawn(async move {
            while let Some(Ok(Message::Text(t))) = read.next().await {
                if let Ok(v) = serde_json::from_str::<Value>(&t) {
                    route(v, &pending_r, &events_r, &seq_r, &dialog_r).await;
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
    /// 阻塞读写跑在游离 std 线程上（不经 `spawn_blocking`）：管道对端
    /// 沉默时 `read`/`write` 会永久阻塞，若挂在 tokio 阻塞池里，runtime
    /// 销毁会等它到天荒地老（e2e  teardown 实测挂死）。游离线程随进程
    /// 退出回收；管道断开（浏览器没了）后自行退出。
    ///
    /// # Errors
    ///
    /// 仅在内部通道装配失败时出错（正常路径无网络 IO）。
    ///
    /// # Panics
    ///
    /// 管道泵里的锁只在「持锁线程先前已 panic」的中毒锁上 panic（实际不可达）。
    pub async fn connect_pipes<R, W>(self: &Arc<Self>, mut read: R, mut write: W) -> Result<()>
    where
        R: std::io::Read + Send + 'static,
        W: std::io::Write + Send + 'static,
    {
        let (tx, mut rx) = mpsc::unbounded_channel::<Value>();
        // 写泵：异步收消息，游离线程做阻塞写（对端不读时 write 会卡，
        // 不能占 tokio 阻塞池名额也不能挡 runtime 销毁）
        let (wbytes_tx, wbytes_rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(256);
        std::thread::Builder::new()
            .name("cdp-pipe-write".into())
            .spawn(move || {
                for mut buf in wbytes_rx {
                    buf.push(b'\0');
                    if write.write_all(&buf).is_err() {
                        break;
                    }
                }
            })
            .expect("pipe write thread");
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if wbytes_tx.send(msg.to_string().into_bytes()).is_err() {
                    break;
                }
            }
        });

        // 读泵：游离线程阻塞读，字节块过通道交异步侧切帧路由
        let pending_r = self.pending.clone();
        let events_r = self.events.clone();
        let seq_r = self.next_seq.clone();
        let dialog_r = self.pending_dialog.clone();
        let flag = self.connected.clone();
        let (chunk_tx, mut chunk_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        std::thread::Builder::new()
            .name("cdp-pipe-read".into())
            .spawn(move || {
                let mut b = [0u8; 8192];
                loop {
                    match read.read(&mut b) {
                        Ok(0) | Err(_) => break, // EOF / 管道断：浏览器侧没了
                        Ok(n) => {
                            if chunk_tx.send(b[..n].to_vec()).is_err() {
                                break;
                            }
                        }
                    }
                }
            })
            .expect("pipe read thread");
        tokio::spawn(async move {
            let mut carry: Vec<u8> = Vec::new();
            while let Some(chunk) = chunk_rx.recv().await {
                carry.extend_from_slice(&chunk);
                while let Some(pos) = carry.iter().position(|&c| c == 0) {
                    let frame: Vec<u8> = carry.drain(..=pos).collect();
                    if let Ok(v) = serde_json::from_slice(&frame[..frame.len() - 1]) {
                        route(v, &pending_r, &events_r, &seq_r, &dialog_r).await;
                    }
                }
            }
            flag.store(false, Ordering::Relaxed);
        });

        *self.outgoing.lock().await = Some(tx);
        self.connected.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// 发一条 CDP 命令并等回应；非 browser 域自动带活动 tab 的 `sessionId`。
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
                 下一步：自起引擎用 browse down 退出"
            )),
            "Browser.setWindowBounds" => Err(anyhow!(
                "browse: 守卫拦截 Browser.setWindowBounds（不动用户窗口）；下一步：无替代操作，调整窗口不属于自动化面"
            )),
            "Target.closeTarget" => {
                let id = params.get("targetId").and_then(Value::as_str).unwrap_or("");
                if self.own_targets.lock().await.contains(id) {
                    Ok(())
                } else {
                    Err(anyhow!(
                        "browse: 守卫拦截 Target.closeTarget：{id} 不是本会话自建 tab；\
                         下一步：只关 listPageTargets()/currentTab() 里 own=true 的 tab；\
                         chrome 启动初始页与用户 tab 不关，用 switchTab 切走即可"
                    ))
                }
            }
            // 域策略（ADR 借鉴 browser-use-pi policy.ts）：deny 优先于 allow，
            // 未配置不拦；命中 host 等值或后缀（x.com 匹配 a.x.com）
            "Page.navigate" | "Target.createTarget" => {
                let url = params.get("url").and_then(Value::as_str).unwrap_or("");
                if let Some(rule) = domain_policy_violation(url) {
                    Err(anyhow!(
                        "browse: 域策略拦截 {method} -> {url}（规则 {rule}）；\
                         下一步：改 BROWSE_ALLOW_DOMAINS/BROWSE_DENY_DOMAINS 或换允许的站点"
                    ))
                } else {
                    Ok(())
                }
            }
            _ => Ok(()),
        }
    }

    /// 仅供引擎自己对 spawn 出来的浏览器做优雅退出：直发 `Browser.close`，绕过守卫。
    ///
    /// 附着来源绝不允许调用（调用方自查来源）。
    ///
    /// # Errors
    ///
    /// 未连接或 CDP 报错。
    pub async fn graceful_close_browser(&self) -> Result<Value> {
        self.send("Browser.close", json!({})).await
    }

    async fn send(&self, method: &str, params: Value) -> Result<Value> {
        let sid = if is_browser_method(method) {
            None
        } else {
            self.session_id.lock().await.clone()
        };
        self.send_with(method, params, sid).await
    }

    /// 显式路由目标的调用，`sessionId` 用给定值（不走活动路由）。
    ///
    /// 给「回执必须回到事件来源 session」的场合：典型是录制的
    /// `Page.screencastFrameAck`，它要应答帧自带的 sessionId。
    ///
    /// # Errors
    ///
    /// 同 [`Session::call`]（守卫、CDP 错误、超时）。
    pub async fn call_on(&self, method: &str, params: Value, session_id: &str) -> Result<Value> {
        self.guard(method, &params).await?;
        self.send_with(method, params, Some(session_id.to_string()))
            .await
    }

    async fn send_with(
        &self,
        method: &str,
        params: Value,
        session_id: Option<String>,
    ) -> Result<Value> {
        if !self.is_connected() {
            return Err(anyhow!("Not connected. Call session.connect(...) first."));
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut msg = json!({ "id": id, "method": method, "params": params });
        if !is_browser_method(method)
            && let Some(sid) = session_id
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
            // 方法名拼错（CDP -32601 not found）：附相近建议（被动增强，不预拦）
            let not_found = err
                .get("code")
                .and_then(Value::as_i64)
                .is_some_and(|c| c == -32601)
                || err
                    .get("message")
                    .and_then(Value::as_str)
                    .is_some_and(|m| m.to_ascii_lowercase().contains("not found"));
            if not_found {
                let near = crate::methods::suggest(method, 3);
                if !near.is_empty() {
                    return Err(anyhow!(
                        "CDP {method}: {err}；下一步：相近方法 {}（全量清单 await cdpMethods(\"<Domain>\")）",
                        near.join(" / ")
                    ));
                }
            }
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

    /// 返回当前活动 sessionId（无则 `None`）。
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
        let own_now = self.own_targets.lock().await.clone();
        let infos = r
            .get("targetInfos")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        Ok(infos
            .into_iter()
            .filter_map(|t| {
                let own = t
                    .get("targetId")
                    .and_then(Value::as_str)
                    .is_some_and(|id| own_now.contains(id));
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
                    own,
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

    /// 断开连接但不关浏览器；`is_connected` 立即为 false，之后可重新
    /// `connect` 或由引擎策略重拉。
    ///
    /// 实现是丢弃发送端，写循环随之退出、socket 关闭。对齐官方 harness
    /// 的 `session.close()`。
    pub async fn close(&self) {
        *self.outgoing.lock().await = None;
        self.connected.store(false, Ordering::Relaxed);
    }

    /// 非破坏窥视事件缓冲：返回 `method` 匹配的前 `n` 条，不消费
    /// （`wait_for` 仍能取到它们）。
    ///
    /// 给 agent 轮询消费事件流用，是官方 `onEvent` 回调在方言（无函数）
    /// 下的等价面。
    ///
    /// 事件自带 [`Session::peek_events_since`] 用的 `seq` 游标。
    pub async fn peek_events(&self, method: &str, n: usize) -> Vec<Value> {
        self.events
            .lock()
            .await
            .iter()
            .filter(|e| e.get("method").and_then(|m| m.as_str()) == Some(method))
            .take(n)
            .cloned()
            .collect()
    }

    /// 增量窥视事件缓冲：只回 `seq > since_seq` 的匹配事件（非破坏）。
    ///
    /// 轮询模式：首拍 `since_seq=0`，之后用上一拍最后一条的 `seq`，
    /// 不重复看旧事件。被环形淘汰的事件自然跳过。
    pub async fn peek_events_since(&self, method: &str, since_seq: u64, n: usize) -> Vec<Value> {
        self.events
            .lock()
            .await
            .iter()
            .filter(|e| {
                e.get("method").and_then(|m| m.as_str()) == Some(method)
                    && e.get("seq")
                        .and_then(Value::as_u64)
                        .is_some_and(|s| s > since_seq)
            })
            .take(n)
            .cloned()
            .collect()
    }

    /// 等值过滤窥视：`method` 匹配且 `path` 点分路径（如 `params.requestId`）
    /// 指到的值 `==` `value` 的前 `n` 条（非破坏）。
    ///
    /// 方言无谓词函数，这是「挑特定 requestId / 特定 frame 的事件」的
    /// 结构化等价面。
    pub async fn find_events(
        &self,
        method: &str,
        path: &str,
        value: &Value,
        n: usize,
    ) -> Vec<Value> {
        self.events
            .lock()
            .await
            .iter()
            .filter(|e| {
                e.get("method").and_then(|m| m.as_str()) == Some(method)
                    && json_path(e, path) == Some(value)
            })
            .take(n)
            .cloned()
            .collect()
    }

    /// 消费式取事件：移除并返回缓冲里全部 `method` 匹配，是 peek 家族
    /// 的破坏性对偶。
    ///
    /// 给常驻消费任务用（录帧泵）；取走后 `waitFor`/`peek` 就见不到这些
    /// 事件了。
    pub async fn drain_events(&self, method: &str) -> Vec<Value> {
        let mut evs = self.events.lock().await;
        let mut out = Vec::new();
        let mut i = 0;
        while i < evs.len() {
            if evs[i].get("method").and_then(|m| m.as_str()) == Some(method) {
                out.push(evs.remove(i).unwrap_or(Value::Null));
            } else {
                i += 1;
            }
        }
        out
    }

    /// 从环形缓冲里等第一个 `method` 事件（取出即移除），超时报错。
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
///
/// 进缓冲的事件盖上单调 `seq`（从 1 起）：`waitFor`/`peek` 拿到的事件自带
/// 游标，供 [`Session::peek_events_since`] 增量轮询。顺带截获对话框事件
/// 维护 [`Session::pending_dialog`]（开着对话框时 Input/evaluate 会挂起，
/// 消费方必须能不等 CDP 就看到它）。
async fn route(
    mut v: Value,
    pending: &Arc<Mutex<HashMap<i64, oneshot::Sender<Value>>>>,
    events: &Arc<Mutex<VecDeque<Value>>>,
    next_seq: &AtomicI64,
    dialog: &Arc<Mutex<Option<Value>>>,
) {
    if let Some(id) = v.get("id").and_then(|x| x.as_i64()) {
        if let Some(tx) = pending.lock().await.remove(&id) {
            let _ = tx.send(v);
        }
    } else if v.get("method").is_some() {
        match v.get("method").and_then(|m| m.as_str()) {
            Some("Page.javascriptDialogOpening") => {
                *dialog.lock().await = Some(v.clone());
            }
            Some("Page.javascriptDialogClosed") => {
                *dialog.lock().await = None;
            }
            _ => {}
        }
        let seq = next_seq.fetch_add(1, Ordering::Relaxed);
        if let Value::Object(map) = &mut v {
            map.insert("seq".into(), json!(seq));
        }
        let mut evs = events.lock().await;
        if evs.len() >= EVENT_BUFFER_CAP {
            evs.pop_front();
        }
        evs.push_back(v);
    }
}

/// 按点分路径取 JSON 子值，如 `json_path(v, "params.requestId")`。
///
/// 段名按对象字段取（空段跳过，空路径返回整值）；路径不存在返回 `None`。
fn json_path<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.')
        .filter(|seg| !seg.is_empty())
        .try_fold(v, |cur, seg| cur.get(seg))
}

/// 域策略命中则返回违规原因（deny 命中 / 不在 allow 清单），否则 `None`。
///
/// 未配置任何规则时恒 `None`；deny 优先于 allow。行为契约见 src 内单元测试。
fn domain_policy_violation(url: &str) -> Option<&'static str> {
    let (allow, deny) = domain_rules();
    if allow.is_empty() && deny.is_empty() {
        return None;
    }
    let host = url_host(url);
    if host.is_empty() {
        return None;
    }
    for rule in &deny {
        if host_matches(&host, rule) {
            return Some("deny 命中");
        }
    }
    if !allow.is_empty() && !allow.iter().any(|r| host_matches(&host, r)) {
        return Some("不在 allow 清单");
    }
    None
}

/// 解析 `BROWSE_ALLOW_DOMAINS` / `BROWSE_DENY_DOMAINS`（逗号分隔，
/// `*.` 前缀与裸域名都按后缀匹配）。
///
/// `'static` 由 leak 一次性换来（进程级配置只解析一次）。
fn domain_rules() -> (Vec<&'static str>, Vec<&'static str>) {
    use std::sync::OnceLock;
    static RULES: OnceLock<(Vec<&'static str>, Vec<&'static str>)> = OnceLock::new();
    RULES
        .get_or_init(|| {
            let parse = |key: &str| -> Vec<&'static str> {
                std::env::var(key)
                    .ok()
                    .map(|v| {
                        v.split(',')
                            .map(|s| s.trim().trim_start_matches("*."))
                            .filter(|s| !s.is_empty())
                            .map(|s| Box::leak(s.to_string().into_boxed_str()) as &'static str)
                            .collect()
                    })
                    .unwrap_or_default()
            };
            (parse("BROWSE_ALLOW_DOMAINS"), parse("BROWSE_DENY_DOMAINS"))
        })
        .clone()
}

/// 裸 URL 的 host 提取（`http(s)://` 后到首个 `/?:#`），不引 url crate。
fn url_host(url: &str) -> String {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    rest.split(['/', ':', '?', '#'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// host 等值或后缀匹配（`x.com` 匹配 `a.x.com`，不匹配 `ax.com`）。
fn host_matches(host: &str, rule: &str) -> bool {
    host == rule || host.strip_suffix(rule).is_some_and(|h| h.ends_with('.'))
}

/// 判断方法是否属于 browser 端点域（这类方法不附 `sessionId`）。
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 回归（2026-09-17 attach 重附不重建缺口）：对端断开后
    /// [`Session::is_connected`] 必须翻 false。旧实现读循环把死线写进
    /// 局部旗，会话永远谎报活着，引擎懒 ensure 因此短路不重连。
    #[tokio::test]
    async fn connected_flag_falls_when_ws_dies() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        // 服务端：完成握手随即丢弃连接（模拟附着 target 换血断线）
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            drop(ws);
        });

        let s = Session::new();
        s.connect(&format!("ws://{addr}")).await.unwrap();
        assert!(s.is_connected(), "握手成功即活着");

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while s.is_connected() && std::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert!(
            !s.is_connected(),
            "ws 断开后 is_connected 必须翻 false（引擎懒 ensure 靠它判定重连）"
        );
    }

    fn ev(method: &str, stamp: f64) -> Value {
        json!({ "method": method, "params": { "timestamp": stamp } })
    }

    /// peek 非破坏：窥视后 wait_for 仍能取到同一条。
    #[tokio::test]
    async fn peek_does_not_consume() {
        let s = Session::new();
        s.events
            .lock()
            .await
            .push_back(ev("Page.loadEventFired", 1.0));
        s.events
            .lock()
            .await
            .push_back(ev("Page.navigatedWithinDocument", 2.0));
        s.events
            .lock()
            .await
            .push_back(ev("Page.loadEventFired", 3.0));

        let peeked = s.peek_events("Page.loadEventFired", 5).await;
        assert_eq!(peeked.len(), 2);
        assert_eq!(
            peeked[0]
                .pointer("/params/timestamp")
                .and_then(Value::as_f64),
            Some(1.0),
            "按入队序取"
        );

        let taken = s
            .wait_for("Page.loadEventFired", 0)
            .await
            .expect("peek 后 wait 仍取得到");
        assert_eq!(
            taken.pointer("/params/timestamp").and_then(Value::as_f64),
            Some(1.0),
            "wait 取的是最早的（peek 没动过缓冲）"
        );
    }

    /// peek 的 n 截断生效。
    #[tokio::test]
    async fn peek_takes_n() {
        let s = Session::new();
        for i in 0..5 {
            s.events
                .lock()
                .await
                .push_back(ev("Network.loadingFailed", i as f64));
        }
        assert_eq!(s.peek_events("Network.loadingFailed", 2).await.len(), 2);
        assert_eq!(s.peek_events("Network.loadingFailed", 0).await.len(), 0);
        assert_eq!(s.peek_events("不存在的.事件", 3).await.len(), 0);
    }

    /// 环形上限：route 丢最老，保留最新。
    #[tokio::test]
    async fn ring_buffer_caps() {
        let s = Session::new();
        let pending = Arc::new(Mutex::new(HashMap::new()));
        for i in 0..(EVENT_BUFFER_CAP + 10) {
            route(
                ev("X.y", i as f64),
                &pending,
                &s.events,
                &s.next_seq,
                &s.pending_dialog,
            )
            .await;
        }
        let len = s.events.lock().await.len();
        assert_eq!(len, EVENT_BUFFER_CAP, "容量封顶");
        let peeked = s.peek_events("X.y", EVENT_BUFFER_CAP + 5).await;
        assert_eq!(
            peeked
                .first()
                .and_then(|v| v.pointer("/params/timestamp"))
                .and_then(Value::as_f64),
            Some(10.0),
            "丢的是最老（0..=9 被挤掉），最旧保留者是 10"
        );
        assert_eq!(
            peeked
                .last()
                .and_then(|v| v.pointer("/params/timestamp"))
                .and_then(Value::as_f64),
            Some((EVENT_BUFFER_CAP + 9) as f64),
            "留的是最新"
        );
    }

    /// close 后 is_connected 立即 false（无需等读循环退出）。
    #[tokio::test]
    async fn close_flips_connected() {
        let s = Session::new();
        s.connected.store(true, Ordering::Relaxed);
        assert!(s.is_connected());
        s.close().await;
        assert!(!s.is_connected());
    }

    async fn route_all(s: &Session, events: Vec<Value>) {
        let pending = Arc::new(Mutex::new(HashMap::new()));
        for e in events {
            route(e, &pending, &s.events, &s.next_seq, &s.pending_dialog).await;
        }
    }

    /// seq 从 1 起单调递增；peekEventsSince 严格大于边界、增量不重看。
    #[tokio::test]
    async fn seq_stamps_and_since_incremental() {
        let s = Session::new();
        route_all(&s, vec![ev("N.a", 1.0), ev("N.a", 2.0), ev("N.a", 3.0)]).await;

        let first = s.peek_events("N.a", 10).await;
        let seqs: Vec<u64> = first
            .iter()
            .map(|e| e.get("seq").and_then(Value::as_u64).unwrap_or(0))
            .collect();
        assert_eq!(seqs, vec![1, 2, 3], "seq 从 1 起单调");

        // since 严格大于：since=1 -> 只见 2,3
        let after1 = s.peek_events_since("N.a", 1, 10).await;
        assert_eq!(after1.len(), 2);
        assert_eq!(
            after1[0]
                .pointer("/params/timestamp")
                .and_then(Value::as_f64),
            Some(2.0)
        );

        // 游标推进到 2 -> 只见 3；到 3 -> 空
        assert_eq!(s.peek_events_since("N.a", 2, 10).await.len(), 1);
        assert_eq!(s.peek_events_since("N.a", 3, 10).await.len(), 0);
        // 不匹配 method 的 since 查询为空
        assert_eq!(s.peek_events_since("别的.事件", 0, 10).await.len(), 0);
    }

    /// drain 消费式：取走后 peek 不再见，非匹配事件保留。
    #[tokio::test]
    async fn drain_consumes_only_matches() {
        let s = Session::new();
        route_all(&s, vec![ev("N.a", 1.0), ev("N.b", 2.0), ev("N.a", 3.0)]).await;
        let drained = s.drain_events("N.a").await;
        assert_eq!(drained.len(), 2);
        assert_eq!(
            s.peek_events("N.a", 10).await.len(),
            0,
            "drain 后 peek 不见"
        );
        assert_eq!(s.peek_events("N.b", 10).await.len(), 1, "非匹配保留");
    }

    /// findEvents：method + 点分路径等值过滤，命中与不命中各验。
    #[tokio::test]
    async fn find_events_by_path_value() {
        let s = Session::new();
        let mk = |rid: &str, status: i64| {
            json!({ "method": "Network.responseReceived",
                    "params": { "requestId": rid, "response": { "status": status } } })
        };
        route_all(
            &s,
            vec![mk("AAA.1", 200), mk("BBB.2", 404), mk("AAA.3", 500)],
        )
        .await;

        let hit = s
            .find_events(
                "Network.responseReceived",
                "params.requestId",
                &json!("AAA.1"),
                5,
            )
            .await;
        assert_eq!(hit.len(), 1);
        assert_eq!(
            hit[0]
                .pointer("/params/response/status")
                .and_then(Value::as_i64),
            Some(200)
        );

        // 嵌套路径 + 数字等值
        let not_found = s
            .find_events(
                "Network.responseReceived",
                "params.response.status",
                &json!(404),
                5,
            )
            .await;
        assert_eq!(not_found.len(), 1);
        assert_eq!(
            not_found[0].get("params").and_then(|p| p.get("requestId")),
            Some(&json!("BBB.2"))
        );

        // 路径不存在 -> 不命中；method 不匹配 -> 不命中
        assert_eq!(
            s.find_events(
                "Network.responseReceived",
                "params.没有这字段",
                &json!(1),
                5
            )
            .await
            .len(),
            0
        );
        assert_eq!(
            s.find_events("别的.事件", "params.requestId", &json!("AAA.1"), 5)
                .await
                .len(),
            0
        );
    }

    /// json_path 点分链与缺失行为。
    #[test]
    fn json_path_walks_and_misses() {
        let v = json!({ "params": { "response": { "status": 301 } } });
        assert_eq!(json_path(&v, "params.response.status"), Some(&json!(301)));
        assert_eq!(json_path(&v, "params"), v.get("params"));
        assert_eq!(json_path(&v, "params.nope.status"), None);
        assert_eq!(json_path(&v, ""), Some(&v));
    }

    /// URL host 提取：协议、端口、路径、大写。
    #[test]
    fn url_host_extraction() {
        assert_eq!(url_host("https://Example.COM/x?y"), "example.com");
        assert_eq!(url_host("http://a.b:8080/"), "a.b");
        assert_eq!(url_host("about:blank"), "about");
        assert_eq!(url_host("data:text/html,x"), "data");
    }

    /// 守卫错误的 CTA 契约：给「下一步」，不只给原因。
    #[tokio::test]
    async fn guard_errors_carry_next_step() {
        let s = Session::new();
        for (method, params, expect) in [
            ("Browser.close", json!({}), "browse down"),
            (
                "Target.closeTarget",
                json!({ "targetId": "NOT-OWN" }),
                "own=true",
            ),
            ("Browser.setWindowBounds", json!({}), "无替代操作"),
        ] {
            let e = s.guard(method, &params).await.expect_err(method);
            let msg = format!("{e:#}");
            assert!(msg.contains("下一步"), "{method} 守卫错误应带 CTA: {msg}");
            assert!(msg.contains(expect), "{method} 应含 {expect}: {msg}");
        }
    }

    /// 后缀匹配语义：子域命中、同级不误伤。
    #[test]
    fn host_suffix_matching() {
        assert!(host_matches("x.com", "x.com"));
        assert!(host_matches("a.x.com", "x.com"));
        assert!(!host_matches("ax.com", "x.com"));
        assert!(!host_matches("x.com.evil.io", "x.com"));
    }
}
