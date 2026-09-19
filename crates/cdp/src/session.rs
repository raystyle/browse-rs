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

/// 提交屏障等待窗毫秒（#34 根因级）：navigate 回执后等主框架
/// frameNavigated 的上限；测试态收短防拖慢单测。
#[cfg(test)]
const BARRIER_WAIT_MS: u64 = 600;
#[cfg(not(test))]
const BARRIER_WAIT_MS: u64 = 5_000;

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
    ///
    /// 旧读泵的退出只在自己仍是当前连接（见 [`Self::conn_epoch`]）时
    /// 才翻旗：重连后旧泵迟到退出不得误杀新连接（批 12 实测：detach
    /// 报 Not connected 即此）。
    connected: Arc<AtomicBool>,
    /// 连接纪元：每次 open_ws/connect_pipes 自增；读泵持有自己的纪元，
    /// 退出时纪元仍等于当前值才翻存活旗。
    conn_epoch: Arc<AtomicI64>,
    next_seq: Arc<AtomicI64>,
    /// 当前打开的 `Page.javascriptDialogOpening` 事件（route 截获维护，
    /// Closed 清空）。对话框会挂起 Input/evaluate，消费方要能先看它。
    pending_dialog: Arc<Mutex<Option<Value>>>,
    /// 换靶时钉住不 detach 的 session 集合（#33 录制保护）：被钉的旧
    /// session 保持附着（如录制中的帧流），事件仍投递由上层过滤。
    pinned_sessions: Arc<Mutex<HashSet<String>>>,
    /// 文档代计数（#30 消注入痕）：sessionId -> 主框架导航次数，由
    /// [`route`] 在 `Page.frameNavigated`（主框架）时递增；同文档跳转
    /// （pushState 走 navigatedWithinDocument）与 iframe 导航不递增。
    /// 快照引用表的代际失效由它支撑，页面侧零写入。
    doc_gens: Arc<Mutex<HashMap<String, u64>>>,
    /// 提交屏障水位（#34 根因级）：sessionId -> 设屏时的事件 seq。
    /// 跨文档 navigate 回执后记录，后续页面级 [`Session::call`] 等
    /// 水位后的主框架 frameNavigated 再放行；满足或超时即清。
    commit_barrier: Arc<Mutex<HashMap<String, u64>>>,
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
            conn_epoch: Arc::new(AtomicI64::new(0)),
            next_seq: Arc::new(AtomicI64::new(1)),
            pending_dialog: Arc::new(Mutex::new(None)),
            pinned_sessions: Arc::new(Mutex::new(HashSet::new())),
            doc_gens: Arc::new(Mutex::new(HashMap::new())),
            commit_barrier: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// 连接是否仍活着：WS/管道读循环存活期间为 true。
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    /// 返回当前连接纪元（每次 open_ws/connect_pipes 自增）：上层用它
    /// 观察重连（如 js_host 的 init 脚本记账随换代清陈尸，全量评审 G1）。
    pub fn connection_epoch(&self) -> i64 {
        self.conn_epoch.load(Ordering::Relaxed)
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
        let gens_r = self.doc_gens.clone();
        let flag = self.connected.clone();
        let dead_pending = self.pending.clone();
        let epoch = self.conn_epoch.fetch_add(1, Ordering::Relaxed) + 1;
        let epoch_r = self.conn_epoch.clone();
        tokio::spawn(async move {
            while let Some(Ok(Message::Text(t))) = read.next().await {
                // 被取代的泵静默退出（评审收口 G）：不再向共享缓冲投递旧
                // 连接的帧，防污染 peekEvents 与误领 browser 级事件
                if epoch_r.load(Ordering::Relaxed) != epoch {
                    break;
                }
                if let Ok(v) = serde_json::from_str::<Value>(&t) {
                    route(v, &pending_r, &events_r, &seq_r, &dialog_r, &gens_r).await;
                }
            }
            // 只有自己仍是当前连接才翻死线：旧连接的泵在重连后迟到退出
            // 不得误杀新连接（批 12 实测：detach 报 Not connected 即此）。
            // 死亡即清在途（全量评审 F1）：pending 永无应答，不清则白等
            // 30 秒还占 eval 单飞槽
            if epoch_r.load(Ordering::Relaxed) == epoch {
                flag.store(false, Ordering::Relaxed);
                for (_, tx) in dead_pending.lock().await.drain() {
                    let _ = tx.send(json!({
                        "error": {"code": -32000, "message": "连接已断开（引擎死亡或重连清账）"}
                    }));
                }
            }
        });

        *self.outgoing.lock().await = Some(tx);
        self.connected.store(true, Ordering::Relaxed);
        self.reset_connection_state().await;
        Ok(())
    }

    /// 新连接落成时清 per-connection 记账（批 6 遗留回收兜底）：钉住
    /// 集合、文档代、提交屏障都是旧连接上旧 session 的状态，重连（引擎
    /// 换代后的再 ensure）后全是陈尸——钉住的 sid 已不存在，忘收场的
    /// 录制不再拖住任何东西。own_targets 是 browser 级（同一浏览器
    /// 重附仍有效），不清。在途调用即时失败（全量评审 F1）：旧连接上的
    /// pending 永无应答，白等 30 秒还占 eval 单飞槽。
    async fn reset_connection_state(&self) {
        let _out = self.outgoing.lock().await;
        self.fail_pending().await;
        self.pinned_sessions.lock().await.clear();
        self.doc_gens.lock().await.clear();
        self.commit_barrier.lock().await.clear();
    }

    /// 把全部在途调用立刻以「连接已断开」失败（全量评审 F1）：给每个
    /// pending 回 error 形响应（走 send_with 的 CDP 错误分支，错误串带
    /// 方法名与归因），比 drop sender 的「cdp dropped」更可诊断。
    async fn fail_pending(&self) {
        for (_, tx) in self.pending.lock().await.drain() {
            let _ = tx.send(json!({
                "error": {"code": -32000, "message": "连接已断开（引擎死亡或重连清账）"}
            }));
        }
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
        let gens_r = self.doc_gens.clone();
        let flag = self.connected.clone();
        let epoch = self.conn_epoch.fetch_add(1, Ordering::Relaxed) + 1;
        let epoch_r = self.conn_epoch.clone();
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
        let dead_pending = self.pending.clone();
        tokio::spawn(async move {
            let mut carry: Vec<u8> = Vec::new();
            while let Some(chunk) = chunk_rx.recv().await {
                // 同 open_ws：被取代的泵静默退出
                if epoch_r.load(Ordering::Relaxed) != epoch {
                    break;
                }
                carry.extend_from_slice(&chunk);
                while let Some(pos) = carry.iter().position(|&c| c == 0) {
                    let frame: Vec<u8> = carry.drain(..=pos).collect();
                    if let Ok(v) = serde_json::from_slice(&frame[..frame.len() - 1]) {
                        route(v, &pending_r, &events_r, &seq_r, &dialog_r, &gens_r).await;
                    }
                }
            }
            // 同 open_ws：旧连接迟到退出不误杀新连接；死亡即清在途
            // （全量评审 F1，同 open_ws 臂）
            if epoch_r.load(Ordering::Relaxed) == epoch {
                flag.store(false, Ordering::Relaxed);
                for (_, tx) in dead_pending.lock().await.drain() {
                    let _ = tx.send(json!({
                        "error": {"code": -32000, "message": "连接已断开（引擎死亡或重连清账）"}
                    }));
                }
            }
        });

        *self.outgoing.lock().await = Some(tx);
        self.connected.store(true, Ordering::Relaxed);
        self.reset_connection_state().await;
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
        // 提交屏障（#34 根因级）：页面级调用先等上一次 navigate 的提交
        // 落定（navigate 自身与 browser 级方法跳过），让 call() 面看到
        // 的是提交后的世界；详见 [`Session::await_commit_barrier`]
        if method != "Page.navigate" && !is_browser_method(method) {
            self.await_commit_barrier().await;
        }
        // navigate 的事件水位在 send 前取：响应续体可能晚于 commit 事件
        // 被调度，send 后再取会把已到的 commit 盖在水位下，屏障永远
        // 不满足、每次导航白等满窗
        let pre_mark = if method == "Page.navigate" {
            Some(self.last_seq().await)
        } else {
            None
        };
        let v = self.send(method, params).await?;
        if method == "Target.createTarget"
            && let Some(id) = v.get("targetId").and_then(Value::as_str)
        {
            self.own_targets.lock().await.insert(id.to_string());
        }
        // Page.navigate 即时递增活动 session 的文档代（#30）：frameNavigated
        // 事件与命令回执的到达序不保证，同步计数消灭「navigate 后紧接
        // 引用旧 ref」的微观竞速窗；事件路径再计一次也无妨（只需不等）。
        // 用户侧导航（点链接、location 跳转）由 route() 的事件计数覆盖。
        if method == "Page.navigate"
            && let Some(sid) = self.session_id.lock().await.clone()
        {
            *self
                .doc_gens
                .lock()
                .await
                .entry(sid.clone())
                .or_insert(0u64) += 1;
            // 跨文档导航（回执带 loaderId、无 errorText、非下载）设提交屏障
            // 水位：后续页面级 call() 等水位后的主框架 frameNavigated 再
            // 放行。同文档导航（fragment）无 loaderId 不设屏障；失败导航
            // （errorText）没有提交不设；下载型导航（isDownload）不换文档
            // 也没有 commit 事件可等，不设（评审 G2，白等满窗）。
            let cross_doc = v
                .get("loaderId")
                .and_then(Value::as_str)
                .is_some_and(|s| !s.is_empty())
                && v.get("errorText")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .is_empty()
                && !v
                    .get("isDownload")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
            if cross_doc && let Some(mark) = pre_mark {
                self.commit_barrier.lock().await.insert(sid, mark);
            }
        }
        Ok(v)
    }

    /// 等待活动 session 的导航提交落定（#34 根因级）：`Page.navigate` 的
    /// 回执先于提交完成，提交窗内的页面级命令要么被拒（captureScreenshot
    /// 的 Not attached）要么落在旧文档（evaluate 的旧 title）。屏障条件是
    /// 水位（navigate 发出前的事件 seq）之后出现该 session 的主框架
    /// `Page.frameNavigated`（commit 信号）。连续导航的前一次提交若恰好
    /// 落在水位后可能提前放行（保守方向，js_host 出口级重试仍兜底）。
    ///
    /// 覆盖面是全部经 [`Session::call`] 的页面级方法（含 waitJs/waitLoad
    /// 等轮询谓词的首拍与每拍）。有界等待（[`BARRIER_WAIT_MS`]，25ms
    /// 轮询），超时放行不报错、留一行 daemon.log（保守：不比无屏障差，
    /// js_host 的出口级重试仍兜底）；满足或超时后清水位，同一窗口只等
    /// 一次。只挂 [`Session::call`]（用户路径）；watcher 走
    /// [`Session::call_on`] 不等（route 应答 requestPaused 不能被拖住）。
    async fn await_commit_barrier(&self) {
        let Some(sid) = self.session_id.lock().await.clone() else {
            return;
        };
        let Some(mark) = self.commit_barrier.lock().await.get(&sid).copied() else {
            return;
        };
        let deadline = tokio::time::Instant::now() + Duration::from_millis(BARRIER_WAIT_MS);
        let landed = loop {
            // 必须用 since 游标取（评审 F：peek 取最旧 N 条，长会话里缓冲
            // 攒下几十次导航后新 commit 永远出窗，屏障名存实亡、每次
            // 导航白等满窗）；since 语义正是 seq > mark
            let landed = self
                .peek_events_since("Page.frameNavigated", mark, 16)
                .await
                .iter()
                .any(|e| {
                    e.get("sessionId").and_then(Value::as_str) == Some(sid.as_str())
                        && e.pointer("/params/frame/parentId").is_none()
                });
            if landed || tokio::time::Instant::now() >= deadline {
                break landed;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        };
        if !landed {
            // 超时留痕（评审 G1）：现场表现为「某些调用偶尔慢」，不留痕
            // 无从归因；daemon.log 可查
            eprintln!(
                "[browse] 提交屏障超时放行（sid {}，水位 seq {mark}，窗 {BARRIER_WAIT_MS}ms 没有 main-frame frameNavigated；常见因：Page 域未开或下载型导航）",
                &sid[..sid.len().min(8)]
            );
        }
        self.commit_barrier.lock().await.remove(&sid);
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
        // 先占 outgoing 锁再登记 pending（全量评审 F1 附带）：与
        // reset_connection_state 的 drain 同序，防「已登记却被清」的
        // 窗口——reset 持 outgoing 时新调用在此排队，drain 只清旧连接
        // 的在途项
        let out_lock = self.outgoing.lock().await;
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        let sent = out_lock
            .as_ref()
            .ok_or_else(|| anyhow!("CDP socket closed"))
            .and_then(|o| o.send(msg).map_err(|_| anyhow!("CDP socket closed")));
        drop(out_lock);
        sent?;

        let resp = match timeout(Duration::from_secs(CALL_TIMEOUT_SECS), rx).await {
            Ok(r) => r.context("cdp dropped")?,
            // 超时清登记（全量评审 F1）：不清则 map 长期积尸；清后迟到的
            // 响应对不上 id，由 route 的 pending-miss 静默丢
            Err(_) => {
                self.pending.lock().await.remove(&id);
                return Err(anyhow!("cdp timeout: deadline has elapsed"));
            }
        };
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
        // 换靶即脱旧附（#33 F1）：陈旧 flat session 的域还开着会重复投递
        // 事件（实测三倍），detach 后事件只剩活动一份；被钉住的（如录制
        // 中，#33）不 detach，保持其后台流
        let old = self.session_id.lock().await.replace(sid.clone());
        let pinned = self.pinned_sessions.lock().await;
        if let Some(old_sid) = old
            && old_sid != sid
            && !pinned.contains(&old_sid)
        {
            drop(pinned);
            if let Err(e) = self
                .send("Target.detachFromTarget", json!({ "sessionId": old_sid }))
                .await
            {
                // 失败留痕不阻断：现场表现为事件重复再现时 daemon.log 有线索
                eprintln!(
                    "[browse] Target.detachFromTarget({}) 失败（忽略）：{e:#}",
                    &old_sid[..old_sid.len().min(8)]
                );
            }
            // 已 detach 的 session 记账随清（批 10 遗留）：doc_gens 与提交
            // 屏障不再累积陈旧 sid；重附会得新 sid，代从 0 重计（旧 ref
            // 本就因 sid 变化作废，无误伤）
            self.doc_gens.lock().await.remove(&old_sid);
            self.commit_barrier.lock().await.remove(&old_sid);
        }
        *self.target_id.lock().await = Some(target_id.to_string());
        Ok(sid)
    }

    /// 钉住一个 session：换靶（[`Session::use_target`]）不再对它
    /// detach，直到 [`Session::unpin_session`]。机制供上层策略用
    /// （如录制中保持帧流跨换靶存活）；钉住期间其事件仍投递，消费方
    /// 自行按 sessionId 过滤。
    ///
    /// 幂等。
    pub async fn pin_session(&self, session_id: &str) {
        self.pinned_sessions
            .lock()
            .await
            .insert(session_id.to_string());
    }

    /// 解钉（幂等）；解钉不主动 detach，下次换靶按常规处理。
    pub async fn unpin_session(&self, session_id: &str) {
        self.pinned_sessions.lock().await.remove(session_id);
    }

    /// 覆写活动 sessionId（高级用法；一般走 [`Session::use_target`]）。
    pub async fn set_active_session(&self, session_id: Option<String>) {
        *self.session_id.lock().await = session_id;
    }

    /// 返回当前活动 sessionId（无则 `None`）。
    pub async fn get_active_session(&self) -> Option<String> {
        self.session_id.lock().await.clone()
    }

    /// 返回该 session 的文档代计数（#30）：主框架导航一次加一，从未见过
    /// 导航事件的 session 返回 0。快照时记现值，引用时对不上即整表作废；
    /// 同文档跳转（pushState）代不动，ref 继续有效。
    pub async fn doc_generation(&self, session_id: &str) -> u64 {
        self.doc_gens
            .lock()
            .await
            .get(session_id)
            .copied()
            .unwrap_or(0)
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
    /// 只认活动 tab 与 browser 级事件（批 6 遗留，对齐 waitForResponse
    /// 的 #33 F2 口径）：事件带 sessionId 时必须等于活动 session，钉住的
    /// 旧 session（录制中）与他 tab 的事件不被误领误消费；无 sessionId
    /// 的 browser 级事件（Target.* 等）不过滤。要看全缓冲（含他 tab）用
    /// [`Session::peek_events`]。
    ///
    /// # Errors
    ///
    /// `wait_ms` 内没等到该事件。
    pub async fn wait_for(&self, method: &str, wait_ms: u64) -> Result<Value> {
        let deadline = tokio::time::Instant::now() + Duration::from_millis(wait_ms);
        loop {
            {
                let active = self.session_id.lock().await.clone();
                let mut evs = self.events.lock().await;
                if let Some(i) = evs.iter().position(|e| {
                    e.get("method").and_then(|m| m.as_str()) == Some(method)
                        && match (&active, e.get("sessionId").and_then(Value::as_str)) {
                            // browser 级事件（无 sessionId）不过滤
                            (_, None) => true,
                            // 无活动 session 时只领 browser 级
                            (None, Some(_)) => false,
                            (Some(a), Some(sid)) => a == sid,
                        }
                }) {
                    return Ok(evs.remove(i).unwrap_or(Value::Null));
                }
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(anyhow!(
                    "等待 {method} 超时；下一步：确认所在域已开（Page 事件先 await session.Page.enable，Network 先 await session.Network.enable），或 peekEvents 看缓冲里已有什么（peek 不过滤他 tab 事件）"
                ));
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
/// 消费方必须能不等 CDP 就看到它）与文档代计数（#30：主框架
/// frameNavigated 递增该 session 的代，供快照引用表做代际失效）。
async fn route(
    mut v: Value,
    pending: &Arc<Mutex<HashMap<i64, oneshot::Sender<Value>>>>,
    events: &Arc<Mutex<VecDeque<Value>>>,
    next_seq: &AtomicI64,
    dialog: &Arc<Mutex<Option<Value>>>,
    gens: &Arc<Mutex<HashMap<String, u64>>>,
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
            // 主框架导航（frame 无 parentId）递增该 session 的文档代（#30）：
            // 主文档被换，旧 backendNodeId 全体作废；iframe 导航（有
            // parentId）不换主文档，同文档跳转走 navigatedWithinDocument，
            // 都不递增。事件须带 sessionId（flat 态页面事件恒有）。
            Some("Page.frameNavigated")
                if v.pointer("/params/frame/parentId").is_none()
                    && v.get("sessionId")
                        .and_then(Value::as_str)
                        .is_some_and(|s| !s.is_empty()) =>
            {
                let sid = v["sessionId"].as_str().unwrap_or_default().to_string();
                *gens.lock().await.entry(sid).or_insert(0u64) += 1;
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

    /// 重连清账（批 12 评审 G1）：二次连接落成后 pinned/doc_gens/屏障
    /// 全清（钉住解钉由 detach 行为可观），own_targets 保留。
    #[cfg(unix)]
    #[tokio::test]
    async fn reconnect_resets_connection_state() {
        use std::io::{Read, Write};
        use std::os::unix::net::UnixStream;
        use std::sync::Mutex;
        // 第一条连接：假对端只回_ack，事件侧灌一条主框架导航
        let (sa, ba) = UnixStream::pair().unwrap();
        let (sb, bb) = UnixStream::pair().unwrap();
        let s = Session::new();
        s.connect_pipes(sa, sb).await.expect("管道连接 1");
        {
            let peer_out = Arc::new(Mutex::new(ba));
            let peer_out_p = peer_out.clone();
            std::thread::spawn(move || {
                let mut bb = bb;
                let mut buf = Vec::<u8>::new();
                let mut chunk = [0u8; 4096];
                loop {
                    let n = match bb.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    buf.extend_from_slice(&chunk[..n]);
                    while let Some(pos) = buf.iter().position(|&b| b == 0) {
                        let frame: Vec<u8> = buf.drain(..=pos).collect();
                        let Ok(v) = serde_json::from_slice::<Value>(&frame[..frame.len() - 1])
                        else {
                            continue;
                        };
                        let Some(id) = v.get("id").cloned() else {
                            continue;
                        };
                        let resp = json!({"id": id, "result": {}});
                        if let Ok(mut out) = peer_out_p.lock() {
                            let _ = out.write_all(serde_json::to_string(&resp).unwrap().as_bytes());
                            let _ = out.write_all(&[0]);
                        }
                    }
                }
            });
            let ev = json!({"method": "Page.frameNavigated",
                "params": {"frame": {"id": "F1"}}, "sessionId": "S1"});
            let mut out = peer_out.lock().unwrap();
            let _ = out.write_all(serde_json::to_string(&ev).unwrap().as_bytes());
            let _ = out.write_all(&[0]);
        }
        s.set_active_session(Some("S1".into())).await;
        // 注：事件先于 set_active 也没关系，route 只看事件自身
        s.pin_session("S1").await;
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(s.doc_generation("S1").await >= 1, "连接 1 上应有文档代");

        // 第二条连接：新对端记录 detach 调用（pin 清空后 use_target 应发 detach）
        let (sa2, ba2) = UnixStream::pair().unwrap();
        let (sb2, bb2) = UnixStream::pair().unwrap();
        let detached = Arc::new(Mutex::new(Vec::<String>::new()));
        let (detached_p, peer_out_p2) = (detached.clone(), Arc::new(Mutex::new(ba2)));
        std::thread::spawn(move || {
            let mut bb2 = bb2;
            let mut buf = Vec::<u8>::new();
            let mut chunk = [0u8; 4096];
            loop {
                let n = match bb2.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                buf.extend_from_slice(&chunk[..n]);
                while let Some(pos) = buf.iter().position(|&b| b == 0) {
                    let frame: Vec<u8> = buf.drain(..=pos).collect();
                    let Ok(v) = serde_json::from_slice::<Value>(&frame[..frame.len() - 1]) else {
                        continue;
                    };
                    let Some(id) = v.get("id").cloned() else {
                        continue;
                    };
                    let method = v.get("method").and_then(Value::as_str).unwrap_or("");
                    let resp = match method {
                        "Target.attachToTarget" => {
                            json!({"id": id, "result": {"sessionId": "S2"}})
                        }
                        "Target.detachFromTarget" => {
                            let sid = v
                                .pointer("/params/sessionId")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string();
                            detached_p.lock().unwrap().push(sid);
                            json!({"id": id, "result": {}})
                        }
                        _ => json!({"id": id, "result": {}}),
                    };
                    if let Ok(mut out) = peer_out_p2.lock() {
                        let _ = out.write_all(serde_json::to_string(&resp).unwrap().as_bytes());
                        let _ = out.write_all(&[0]);
                    }
                }
            }
        });
        s.connect_pipes(sa2, sb2).await.expect("管道连接 2");
        assert_eq!(s.doc_generation("S1").await, 0, "重连后文档代应清零");
        // 钉住集已清：use_target 换靶会对旧 sid 发 detach（不清则跳过）
        s.use_target("T2").await.expect("attach T2");
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(
            detached.lock().unwrap().iter().any(|sid| sid == "S1"),
            "pin 清空后旧 sid 应被 detach: {:?}",
            detached.lock().unwrap()
        );
    }

    /// 连接死亡即在途调用立刻失败（全量评审 F1）：不留 30 秒白等与
    /// eval 单飞槽独占；错误归因是「连接已断开」不是「超时」。
    #[cfg(unix)]
    #[tokio::test]
    async fn connection_death_fails_inflight_calls() {
        use std::io::Read;
        use std::os::unix::net::UnixStream;
        use std::sync::Mutex;
        let (sa, ba) = UnixStream::pair().unwrap();
        let (sb, bb) = UnixStream::pair().unwrap();
        let s = Session::new();
        s.connect_pipes(sa, sb).await.expect("管道连接");
        s.set_active_session(Some("S1".into())).await;
        // 假对端：收下请求但永不回应；对端写句柄可被测试侧收回以断连
        let peer_out = Arc::new(Mutex::new(Some(ba)));
        let peer_out_p = peer_out.clone();
        std::thread::spawn(move || {
            let mut bb = bb;
            let mut buf = Vec::<u8>::new();
            let mut chunk = [0u8; 4096];
            loop {
                let n = match bb.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                buf.extend_from_slice(&chunk[..n]);
                while let Some(pos) = buf.iter().position(|&b| b == 0) {
                    buf.drain(..=pos);
                    // 故意不回应
                }
            }
            drop(peer_out_p);
        });
        let t0 = tokio::time::Instant::now();
        let call = tokio::spawn({
            let s = s.clone();
            async move {
                s.call("Runtime.evaluate", json!({ "expression": "1" }))
                    .await
            }
        });
        // 让调用先发起（已登记 pending、已上线），再掐断连接
        tokio::time::sleep(Duration::from_millis(200)).await;
        *peer_out.lock().unwrap() = None; // 关对端写句柄 -> 我方读端 EOF
        let r = tokio::time::timeout(Duration::from_millis(2_000), call)
            .await
            .expect("连接死亡应在 2 秒内令在途调用失败（修前挂满 30 秒）")
            .expect("task 不应 panic");
        let msg = format!("{r:#?}");
        assert!(
            msg.contains("连接已断开"),
            "错误应归因连接断开而非超时: {msg}"
        );
        assert!(
            t0.elapsed() < Duration::from_millis(2_500),
            "失败应即时（实测 {:?}）",
            t0.elapsed()
        );
    }

    /// waitFor 活动过滤（批 6 遗留）：他 sid 的同 method 事件不被误领，
    /// browser 级（无 sessionId）不过滤。
    #[tokio::test]
    async fn wait_for_filters_foreign_sessions() {
        let s = Session::new();
        s.events.lock().await.push_back(json!({
            "method": "Page.frameNavigated", "params": {}, "sessionId": "OTHER", "seq": 1
        }));
        s.set_active_session(Some("S1".into())).await;
        let got = tokio::time::timeout(Duration::from_millis(300), async {
            // 后台线程 150ms 后投活动 session 的事件（先用泵不进，走直塞
            // 需要锁；经 spawn 的管道路径过重，这里由定时任务直塞）
            tokio::time::sleep(Duration::from_millis(150)).await;
            s.events.lock().await.push_back(json!({
                "method": "Page.frameNavigated", "params": {}, "sessionId": "S1", "seq": 2
            }));
            Ok::<(), ()>(())
        });
        let _ = got.await;
        let ev = s
            .wait_for("Page.frameNavigated", 2_000)
            .await
            .expect("等到活动事件");
        assert_eq!(ev.get("sessionId").and_then(Value::as_str), Some("S1"));
        // 他 sid 的事件仍在缓冲（peek 可见），且 browser 级不过滤
        assert_eq!(
            s.peek_events("Page.frameNavigated", 5).await.len(),
            1,
            "他 sid 留缓冲"
        );
        s.events.lock().await.push_back(json!({
            "method": "Target.targetCreated", "params": {}, "seq": 3
        }));
        let ev = s
            .wait_for("Target.targetCreated", 300)
            .await
            .expect("browser 级不过滤");
        assert!(ev.get("sessionId").is_none());
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
                &s.doc_gens,
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
            route(
                e,
                &pending,
                &s.events,
                &s.next_seq,
                &s.pending_dialog,
                &s.doc_gens,
            )
            .await;
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

    /// 提交屏障超时放行（#34 根因级）：跨文档 navigate 后 commit 事件
    /// 永不到（Page 域未开的场），页面级调用在屏障窗（本 crate 测试态
    /// 600ms，cfg(test) 跨 crate 不生效故测在 cdp）耗尽后照常发出不报错。
    #[cfg(unix)]
    #[tokio::test]
    async fn commit_barrier_timeout_proceeds() {
        use std::io::{Read, Write};
        use std::os::unix::net::UnixStream;
        use std::sync::Mutex;
        let (sa, ba) = UnixStream::pair().unwrap();
        let (sb, bb) = UnixStream::pair().unwrap();
        let s = Session::new();
        s.connect_pipes(sa, sb).await.expect("管道连接");
        s.set_active_session(Some("S1".into())).await;
        let peer_out = Arc::new(Mutex::new(ba));
        let peer_out_p = peer_out.clone();
        std::thread::spawn(move || {
            let mut bb = bb;
            let mut buf = Vec::<u8>::new();
            let mut chunk = [0u8; 4096];
            loop {
                let n = match bb.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                buf.extend_from_slice(&chunk[..n]);
                while let Some(pos) = buf.iter().position(|&b| b == 0) {
                    let frame: Vec<u8> = buf.drain(..=pos).collect();
                    let Ok(v) = serde_json::from_slice::<Value>(&frame[..frame.len() - 1]) else {
                        continue;
                    };
                    let Some(id) = v.get("id").cloned() else {
                        continue;
                    };
                    // navigate 恒带 loaderId；永不发 commit 事件
                    let resp = if v.get("method").and_then(Value::as_str) == Some("Page.navigate") {
                        json!({"id": id, "result": {"frameId": "F1", "loaderId": "L1"}})
                    } else {
                        json!({"id": id, "result": {}})
                    };
                    if let Ok(mut out) = peer_out_p.lock() {
                        let _ = out.write_all(serde_json::to_string(&resp).unwrap().as_bytes());
                        let _ = out.write_all(&[0]);
                    }
                }
            }
        });
        s.call("Page.navigate", json!({ "url": "http://stuck.test/" }))
            .await
            .expect("navigate");
        let t0 = tokio::time::Instant::now();
        s.call("Page.enable", json!({}))
            .await
            .expect("屏障耗尽后应放行");
        let waited = t0.elapsed();
        assert!(
            waited >= Duration::from_millis(550),
            "屏障窗内应等待（实测 {waited:?}）"
        );
        assert!(
            waited < Duration::from_millis(2_500),
            "不应远超屏障窗（实测 {waited:?}）"
        );
    }
}
