//! 方言求值器：把 [`crate::parser`] 的语句树跑在宿主侧。
//!
//! 移植自 browser-harness-rs `src/js_host.rs` 的求值半边：
//!
//! - `session.<Domain>.<method>(params)` 直接转发 CDP 字符串调用（无 652 个
//!   typed wrapper，新 Chrome 方法不用 codegen）。
//! - 宿主全局：`listPageTargets()`、`resolveWsUrl(opts?)`、`print(x)`。
//! - `session.connect / use / waitFor / call / isConnected / getActiveSession`。
//! - `vars` 在 daemon 内跨片段持久（`const tabs = ...` 之后的片段还能用 `tabs`）。

use crate::parser::{Expr, Stmt, parse_script};

/// 密钥仓（#25.4）：`--secrets <dotenv>` 加载一次进进程级全局；方言经
/// `secrets.<NAME>` 取值，渲染与报错面统一走 [`mask_secrets`] 脱敏。
static SECRETS: std::sync::Mutex<Vec<(String, String)>> = std::sync::Mutex::new(Vec::new());

/// 密钥仓测试串行锁（全 crate 测试可见）：SECRETS 是进程级全局，js_host
/// 与 server 的密钥相关测试并行互踩；tokio Mutex 可跨 await 持守卫。
#[cfg(test)]
pub(crate) static SECRETS_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// 加载 dotenv 形密钥文件（#25.4）：`KEY=VALUE` 行，`#` 注释与空行忽略，
/// 键容 `export ` 前缀（shell 直用同文件，批 9 G2），值剥首尾配对引号、
/// 剥行内注释（未引值从首个 ` #` 截断；引号值以闭引号为界，`#` 在引号
/// 内是字面）；重复键后者覆盖。幂等（清后装）。
///
/// # Errors
///
/// 文件读不了或某行不是 `KEY=VALUE` 形（错误带行号）。
pub fn load_secrets(path: &str) -> Result<()> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| anyhow!("读不了密钥文件 {path}：{e}；下一步：检查 --secrets 路径与权限"))?;
    // 剥 UTF-8 BOM（评审 G1）：Windows 记事本默认写 BOM，不剥则首键带
    // \u{feff} 前缀查无且报错不指向 BOM
    let text = text.trim_start_matches('\u{feff}');
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            bail!(
                "密钥文件第 {} 行不是 KEY=VALUE 形：{line}；下一步：每行一个键值对，# 注释",
                i + 1
            );
        };
        // export 前缀容错（批 9 G2，评审追加固）：shell 与 browse 共用一份
        // 文件；「export」后接空白（空格或制表符）才剥，防 exporter= 被误伤
        let k = k.trim();
        let k = match k.strip_prefix("export") {
            Some(rest) if rest.is_empty() || rest.starts_with(char::is_whitespace) => {
                rest.trim_start()
            }
            _ => k,
        }
        .to_string();
        let mut v = v.trim().to_string();
        // 行内注释（批 9 G2）：引号值闭引号后为注释界；未引值从首个 " #"
        // 截断（# 前无空格是字面，dotenv 通例）
        let quoted = v.starts_with('"') || v.starts_with('\'');
        if quoted {
            let q = v.as_bytes()[0] as char;
            if let Some(close) = v[1..].find(q) {
                v = v[..close + 2].to_string();
            }
        } else if let Some(cut) = v.find(" #") {
            v.truncate(cut);
            v = v.trim_end().to_string();
        }
        if v.len() >= 2
            && ((v.starts_with('"') && v.ends_with('"'))
                || (v.starts_with('\'') && v.ends_with('\'')))
        {
            v = v[1..v.len() - 1].to_string();
        }
        out.retain(|(ek, _): &(String, String)| ek != &k);
        out.push((k, v));
    }
    if let Ok(mut g) = SECRETS.lock() {
        *g = out;
    }
    Ok(())
}

/// 对错误/回显串做子串脱敏（#25.4 评审 G4）：只换密钥值出现处，保留
/// 其余文本（CTA 不能整串换没）；无密钥或不含则原样。
pub fn mask_secrets_str(s: &str) -> String {
    let Ok(secrets) = SECRETS.lock() else {
        return s.to_string();
    };
    let mut out = s.to_string();
    for (_, val) in secrets.iter() {
        if !val.is_empty() && out.contains(val.as_str()) {
            out = out.replace(val.as_str(), "***");
        }
    }
    out
}

/// 对值做脱敏（#25.4）：字符串里出现任何密钥值即整值换 `***`（保守全换，
/// 不做部分遮挡，防切片还原）；非字符串原样。
pub fn mask_secrets(v: &Value) -> Value {
    // 先持锁取判定、放锁再递归：std Mutex 不可重入，持锁递归在嵌套
    // 容器上自死锁（本批实弹雷）
    let hit = {
        let Ok(secrets) = SECRETS.lock() else {
            return v.clone();
        };
        !secrets.is_empty()
            && match v {
                Value::String(s) => secrets
                    .iter()
                    .any(|(_, val)| !val.is_empty() && s.contains(val.as_str())),
                _ => false,
            }
    };
    if hit {
        return Value::String("***".into());
    }
    match v {
        Value::Array(a) => Value::Array(a.iter().map(mask_secrets).collect()),
        Value::Object(o) => Value::Object(
            o.iter()
                .map(|(k, v)| (k.clone(), mask_secrets(v)))
                .collect(),
        ),
        other => other.clone(),
    }
}
use anyhow::{Result, anyhow, bail};
use cdp::methods::METHODS_RAW;
use cdp::{ConnectOptions, PageTarget, Session};
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tokio::sync::Mutex;

/// 方言宿主：持有一个 CDP [`Session`] 加一份跨片段持久的变量表。
pub struct JsHost {
    session: Arc<Session>,
    vars: Mutex<HashMap<String, Value>>,
    /// 元素引用表（D35-lite）：最近一次 `snapshot()` 的短 ref -> backendNodeId，
    /// 连同快照时的 session 与文档代（daemon 侧代际失效，#30 消注入痕）。
    /// 整表随 snapshot 替换。
    refs: Mutex<Option<RefTable>>,
    /// 进行中的录制（至多一场；方言面 recordStart/recordStop 管理）。
    record: Mutex<Option<crate::record::Recorder>>,
    /// 网络拦截规则（routeBlock/routeMock 管理，watcher 应答 requestPaused）。
    routes: Arc<Mutex<Vec<RouteRule>>>,
    /// Fetch 域是否由本宿主开启（手动 `session.Fetch.enable` 不被 watcher 打扰）。
    fetch_by_us: Arc<std::sync::atomic::AtomicBool>,
    /// 每新文档注入脚本（#25.2 setInitScript）：watcher 对每个新 session
    /// 补注（对齐 Page.enable 的兜底节奏）。
    init_script: Arc<std::sync::Mutex<Option<String>>>,
    /// 已注册 init 脚本的 identifier 记账（#25.2 评审 F1）：sessionId ->
    /// identifier 集合（即时路径与 watcher 可能各注一次，都要记账）；
    /// 替换/清除先 removeScriptToEvaluateOnNewDocument 再 add，否则旧注册
    /// 叠加且注册跨 reload 永不回收。
    init_script_ids: Arc<Mutex<HashMap<String, Vec<String>>>>,
}

/// 一条网络拦截规则：glob 模式（`*` 通配）+ 命中动作。
#[derive(Clone)]
pub(crate) struct RouteRule {
    pattern: String,
    action: RouteAction,
}

/// 命中后的动作：直接失败（BlockedByClient）或本地应答。
#[derive(Clone)]
pub(crate) enum RouteAction {
    /// `Fetch.failRequest`（BlockedByClient）。
    Block,
    /// `Fetch.fulfillRequest` 本地应答：body/status/contentType。
    Mock {
        /// 应答体（UTF-8）。
        body: String,
        /// HTTP 状态码（缺省 200）。
        status: i64,
        /// Content-Type（缺省 text/html）。
        content_type: String,
        /// 额外响应头（#38：Content-Disposition 触发下载等）。
        headers: Vec<(String, String)>,
    },
}

/// 引用表：短 ref -> backendNodeId，加 snapshot 时刻的 session 与文档代
/// （daemon 侧计数，#30 消注入痕）。换 tab（backendNodeId 跨 target 无
/// 意义）或主框架导航（代递增）即整表作废；SPA 同文档跳转（pushState
/// 走 navigatedWithinDocument，代不动）ref 继续有效；比 URL 对比零假阳性。
#[derive(Clone)]
struct RefTable {
    session_id: String,
    generation: u64,
    map: HashMap<String, RefEntry>,
}

/// 一条 ref 的落点（#60）：backendNodeId 加可选的「归属子 session」——
/// OOPIF 节点的 backendNodeId 只在它自己的子 session 里有意义（量布局、
/// 填值都要回到那条 session 上做），None 表示顶层活动 session。
#[derive(Clone)]
struct RefEntry {
    backend_node_id: i64,
    session: Option<String>,
}

impl JsHost {
    /// 绑定一条会话建宿主；变量表为空，随 daemon 生命期累积。
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # // no_run：构造会 spawn 对话框与路由 watcher 任务，需要 tokio runtime
    /// let host = browse_core::JsHost::new(cdp::Session::new());
    /// ```
    pub fn new(session: Arc<Session>) -> Arc<Self> {
        let init_script: Arc<std::sync::Mutex<Option<String>>> =
            Arc::new(std::sync::Mutex::new(None));
        let init_script_ids: Arc<Mutex<HashMap<String, Vec<String>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        spawn_dialog_watcher(
            session.clone(),
            init_script.clone(),
            init_script_ids.clone(),
        );
        let routes = Arc::new(Mutex::new(Vec::new()));
        let fetch_by_us = Arc::new(std::sync::atomic::AtomicBool::new(false));
        spawn_route_watcher(session.clone(), routes.clone(), fetch_by_us.clone());
        Arc::new(Self {
            session,
            vars: Mutex::new(HashMap::new()),
            refs: Mutex::new(None),
            record: Mutex::new(None),
            routes,
            fetch_by_us,
            init_script,
            init_script_ids,
        })
    }

    /// 规则变更后同步 `Fetch.enable` 的 pattern 集（去重；空则 disable）。
    ///
    /// 拦截是 per-session 的：规则作用于当前活动 tab，换 tab 后重设规则。
    async fn sync_fetch_patterns(&self) -> Result<()> {
        let rules = self.routes.lock().await.clone();
        if rules.is_empty() {
            if self.fetch_by_us.load(Ordering::Relaxed) {
                self.session
                    .call("Fetch.disable", json!({}))
                    .await
                    .map_err(|e| e.context("Fetch.disable"))?;
                self.fetch_by_us.store(false, Ordering::Relaxed);
            }
            return Ok(());
        }
        let mut seen = std::collections::BTreeSet::new();
        let patterns: Vec<Value> = rules
            .iter()
            .map(|r| r.pattern.clone())
            .filter(|p| seen.insert(p.clone()))
            .map(|p| json!({ "urlPattern": p }))
            .collect();
        self.session
            .call("Fetch.enable", json!({ "patterns": patterns }))
            .await
            .map_err(|e| e.context("Fetch.enable"))?;
        self.fetch_by_us.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// AX 树投影（#36）：getFullAXTree 拉全量，投影成扁平节点表（丢纯
    /// 布局节点，判据同旧 snapshot），childIds 随卷保留并另建 parent 边
    /// 供限深、子树、祖先链过滤。提交窗竞速同走 call_commit_retry。
    ///
    /// # Errors
    ///
    /// 同 [`Session::call`]（含提交窗重试耗尽）。
    /// annotate 失败路径的静默清场（#41 评审 F2）：不留半批框污染页面，
    /// 清场自身的失败也不掩盖主错误。
    async fn highlight_clear_quiet(&self) {
        let _ = self
            .session
            .call(
                "Runtime.evaluate",
                json!({
                    "expression": "(() => { document.querySelectorAll('[data-browse-hl]').forEach(n => n.remove()); return true })()",
                    "returnByValue": true
                }),
            )
            .await;
    }

    /// DOM 穿透投影（#47）：getDocument depth -1 pierce true 走
    /// contentDocument（同源 iframe）与 shadowRoots；元素节点投影为
    /// {id, role=nodeName, name=可见文本/id/aria, backendNodeId, frameId 无
    /// （DOM 节点不带，AX 面才有）}。与 AX 投影同名节点由 ref 去重面
    /// 合并（ref 盖章时 backendNodeId 相同即同节点自然去重做不到——由
    /// 调用方接受重复，ref 唯一性由计数器保）。
    async fn pierce_dom_nodes(&self) -> Result<Vec<Value>> {
        let r = self
            .call_commit_retry("DOM.getDocument", json!({ "depth": -1, "pierce": true }))
            .await?;
        let mut out = Vec::new();
        let root = r.get("root").cloned().unwrap_or(json!({}));
        Self::walk_pierced(&root, &mut out, 0);
        Ok(out)
    }

    fn walk_pierced(n: &Value, out: &mut Vec<Value>, depth: usize) {
        if depth > 24 {
            return;
        }
        let name_tag = n.get("nodeName").and_then(Value::as_str).unwrap_or("");
        if let Some(bn) = n.get("backendNodeId").and_then(Value::as_i64) {
            // 元素节点才投影（#text 等无 bn 的跳过；nodeName #开头的是非元素）
            if !name_tag.starts_with('#') {
                let attrs = n.get("attributes").cloned().unwrap_or(json!([]));
                let idv = attrs
                    .as_array()
                    .and_then(|a| a.iter().position(|x| x == &json!("id")))
                    .and_then(|p| attrs.as_array().and_then(|a| a.get(p + 1)).cloned())
                    .and_then(|v| v.as_str().map(str::to_string))
                    .unwrap_or_default();
                let aria = attrs
                    .as_array()
                    .and_then(|a| a.iter().position(|x| x == &json!("aria-label")))
                    .and_then(|p| attrs.as_array().and_then(|a| a.get(p + 1)).cloned())
                    .and_then(|v| v.as_str().map(str::to_string))
                    .unwrap_or_default();
                let name = if !aria.is_empty() { aria } else { idv };
                out.push(json!({
                    "id": bn,
                    "role": name_tag.to_lowercase(),
                    "name": name,
                    "frameId": Value::Null,
                    "value": Value::Null,
                    "backendNodeId": bn,
                }));
            }
        }
        for key in ["children", "shadowRoots"] {
            if let Some(cs) = n.get(key).and_then(Value::as_array) {
                for c in cs {
                    Self::walk_pierced(c, out, depth + 1);
                }
            }
        }
        if let Some(cd) = n.get("contentDocument") {
            Self::walk_pierced(cd, out, depth + 1);
        }
    }

    async fn ax_projected(&self) -> Result<(Vec<Value>, HashMap<i64, i64>)> {
        let r = self
            .call_commit_retry("Accessibility.getFullAXTree", json!({}))
            .await?;
        let mut parent: HashMap<i64, i64> = HashMap::new();
        let nodes: Vec<Value> = r
            .get("nodes")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|n| {
                let role = n
                    .pointer("/role/value")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if matches!(role, "generic" | "InlineTextBox" | "presentation" | "none") {
                    return None;
                }
                let name = n
                    .pointer("/name/value")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if name.is_empty()
                    && !n.get("value").is_some_and(|v| !v.is_null())
                    && n.get("backendDOMNodeId").is_none()
                {
                    return None;
                }
                // nodeId/childIds 在真 chrome 回执里是数字串（"2"），假对端
                // 与文档示例是数字——两形都吃（#36 实弹坑）
                let id = n.get("nodeId").and_then(ax_id)?;
                for c in n
                    .get("childIds")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .filter_map(ax_id)
                {
                    parent.insert(c, id);
                }
                // frameId 透传（#47）：AX 节点原生带，同源 iframe 内容在
                // getFullAXTree 里以子树出现（实弹），跨 frame ref 表生效
                Some(json!({
                    "id": id,
                    "role": role,
                    "name": name,
                    "frameId": n.get("frameId"),
                    "value": n.get("value").and_then(|v| v.get("value")),
                    "checked": n.get("checked"),
                    "pressed": n.get("pressed"),
                    "selected": n.get("selected"),
                    "expanded": n.get("expanded"),
                    "disabled": n.get("disabled"),
                    "backendNodeId": n.get("backendDOMNodeId"),
                    "childIds": n.get("childIds"),
                }))
            })
            .collect();
        Ok((nodes, parent))
    }

    /// 等 URL 命中 glob 的最近一个响应完成（#20）：窗口语义是「最近命中
    /// （含历史，前提是 Network 域在触发前已开），没有则等到超时」；方言
    /// 无并发，可用形态是触发后等待。命中响应头后等同一 requestId 的体
    /// 完成信号（loadingFinished，体窗 5 秒）再取
    /// `Network.getResponseBody`（base64 自动解码），可解析为 JSON 时附
    /// `json` 字段；体取失败显式 `body: null` 加 `bodyError`，不静默省略。
    ///
    /// # Errors
    ///
    /// - `Network.enable` 失败（未连接引擎）。
    /// - 超时窗内没有命中响应（错误带 pattern 与 glob 写法 CTA）。
    async fn wait_for_response(&self, pattern: &str, ms: u64) -> Result<Value> {
        // 开 Network 域（幂等）：不开收不到响应事件；全链错误保 CTA
        self.session
            .call("Network.enable", json!({}))
            .await
            .map_err(|e| anyhow!("Network.enable 失败：{e:#}"))?;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(ms);
        // 只认活动 tab 的事件（#33 F2）：他 tab 的响应元数据不误领，
        // 陈旧 session 的重复副本（detach 前残影）也一并排除
        let active = self.session.get_active_session().await;
        let from_active = |e: &Value| match &active {
            Some(a) => e.get("sessionId").and_then(Value::as_str) == Some(a.as_str()),
            None => e.get("sessionId").is_none(),
        };
        let ev = loop {
            let hits = self
                .session
                .peek_events("Network.responseReceived", 1000)
                .await;
            if let Some(e) = hits.into_iter().rev().find(|e| {
                e.pointer("/params/response/url")
                    .and_then(Value::as_str)
                    .is_some_and(|u| glob_match(pattern, u))
                    && from_active(e)
            }) {
                break e;
            }
            if tokio::time::Instant::now() >= deadline {
                // CTA 清单封顶（#33 F3）：去重后只列最近几条加总数，
                // 不把整窗 URL 灌进上下文
                let mut uniq: Vec<String> = Vec::new();
                let mut total = 0usize;
                for u in self
                    .session
                    .peek_events("Network.responseReceived", 1000)
                    .await
                    .iter()
                    .filter(|e| from_active(e))
                    .filter_map(|e| {
                        e.pointer("/params/response/url")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    })
                {
                    total += 1;
                    if !uniq.contains(&u) {
                        uniq.push(u);
                    }
                }
                let sample: Vec<String> = uniq.iter().rev().take(3).cloned().collect();
                bail!(
                    "waitForResponse 超时（{} 秒内活动 tab 没有 URL 命中 {pattern} 的响应；活动窗 {} 条 responseReceived，去重 {} 条，最近有 {sample:?}）；下一步：pattern 与 routeBlock/routeMock 同一套 glob 写法（* 通配），确认动作发生在活动 tab 且在超时窗内，必要时先 await session.Network.enable({{}})；当前没有活动 tab 时先 await session.use(tabs[0].targetId)",
                    ms / 1000,
                    total,
                    uniq.len()
                );
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        };
        let rid = ev
            .pointer("/params/requestId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let mut out = json!({
            "requestId": rid,
            "url": ev.pointer("/params/response/url").cloned().unwrap_or(Value::Null),
            "status": ev.pointer("/params/response/status").cloned().unwrap_or(Value::Null),
            "headers": ev.pointer("/params/response/headers").cloned().unwrap_or(Value::Null),
        });
        if !rid.is_empty() {
            // 体就绪等待（评审 F）：responseReceived 只到响应头，等同一
            // requestId 的 loadingFinished/loadingFailed（体窗 5 秒）再取，
            // 防头到体未就绪时静默丢 body
            let rid_ref = rid.as_str();
            let body_ready = |evs: &[Value]| {
                evs.iter().any(|e| {
                    e.pointer("/params/requestId").and_then(Value::as_str) == Some(rid_ref)
                })
            };
            let body_deadline =
                tokio::time::Instant::now() + std::time::Duration::from_millis(BODY_WAIT_MS);
            loop {
                let done = body_ready(
                    &self
                        .session
                        .peek_events("Network.loadingFinished", 1000)
                        .await,
                ) || body_ready(
                    &self
                        .session
                        .peek_events("Network.loadingFailed", 1000)
                        .await,
                );
                if done || tokio::time::Instant::now() >= body_deadline {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            match self
                .session
                .call("Network.getResponseBody", json!({ "requestId": rid }))
                .await
            {
                Ok(b) => {
                    let decoded = decode_response_body(&b);
                    if let Value::String(text) = &decoded["body"]
                        && let Ok(parsed) = serde_json::from_str::<Value>(text)
                    {
                        out["json"] = parsed;
                    }
                    out["base64Encoded"] = decoded["base64Encoded"].clone();
                    out["body"] = decoded["body"].clone();
                }
                Err(e) => {
                    // 体失败显式化：body 为 null 加 bodyError 带因，不静默省略
                    out["body"] = Value::Null;
                    out["bodyError"] = json!(format!("{e:#}"));
                }
            }
        }
        Ok(out)
    }

    /// 带提交窗重试的页面级调用（#34）：navigate 回执先于导航提交完成，
    /// 提交窗内的页面级命令会报「Not attached to an active page」（同片段
    /// navigate 后首个动作的竞速窗）。对该错误串有界重试（100ms 步进至多
    /// 2 秒），其余错误原样上抛；耗尽时附提交窗辨识提示再交最后错误
    /// （评审 G2：原文可诊断，提示给归因）。screenshot 与 snapshot 的 AX
    /// 拉取同用（评审 G1：竞速是出口共面的，不只截图）。
    async fn call_commit_retry(&self, method: &str, params: Value) -> Result<Value> {
        for attempt in 0..20u32 {
            match self.session.call(method, params.clone()).await {
                Ok(v) => return Ok(v),
                Err(e) => {
                    let transient = format!("{e:#}").contains("Not attached to an active page");
                    if !transient {
                        return Err(e);
                    }
                    if attempt == 19 {
                        return Err(e.context(
                            "重试 2 秒仍 Not attached（可能导航提交窗超预算：大页或慢机）；下一步：分两片段发（先 navigate 后动作）或稍候重试",
                        ));
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            }
        }
        unreachable!("重试循环必有返回")
    }

    /// 交互/求值族调用前的快失败：有未处理的 confirm/prompt 时 Input 与
    /// Runtime.evaluate 都会挂起，与其烧超时不如立刻给 CTA。
    async fn assert_no_dialog(&self) -> Result<()> {
        if self.session.pending_dialog().await.is_some() {
            bail!(
                "页面有未处理的 confirm/prompt 对话框（Input 会被它挂起）；下一步：return await dialogStatus() 看内容，再 await dialogAccept() 或 await dialogDismiss()"
            );
        }
        Ok(())
    }

    /// 返回宿主共享的会话（health/status 面用）。
    pub fn session(&self) -> Arc<Session> {
        self.session.clone()
    }

    /// 求值一段方言片段，返回最后一条语句的值（或 `return` 的值）。
    ///
    /// # Errors
    ///
    /// - 语法错（[`crate::parser::parse_script`] 的错误透传）。
    /// - 未定义变量、未知函数、非法调用。
    /// - CDP 调用失败（含 [`Session::call`] 的守卫拦截与超时）。
    pub async fn eval_snippet(&self, source: &str) -> Result<Value> {
        let stmts = parse_script(source)?;
        let mut last = Value::Null;
        for st in stmts {
            match st {
                Stmt::Let { name, expr } => {
                    let v = self.eval_expr(&expr).await?;
                    self.vars.lock().await.insert(name, v.clone());
                    last = v;
                }
                Stmt::Expr(expr) => {
                    last = self.eval_expr(&expr).await?;
                }
                Stmt::Return(expr) => {
                    return self.eval_expr(&expr).await;
                }
            }
        }
        Ok(last)
    }

    /// 全量 JS 一次求值（#22 受限旁路，ADR-0002 修订）：不经方言解析器，
    /// 直发 `Runtime.evaluate`（returnByValue 加 awaitPromise），表达式
    /// 值序列化回传。分工口径：方言管 CDP 编排与宿主便捷函数，本面与
    /// 方言全局 `pageEval(js)`（同一能力的方言内形态）管页面逻辑（模板
    /// 字符串、正则、函数声明等真 V8 语法直接写）。
    ///
    /// 顶层 `return` 桥（#22）：先原样发，SyntaxError 认出「Illegal
    /// return statement」后包 async IIFE（换行定界）重发一次——方言习惯
    /// （声明几条加末尾 return，任意位置）不罚脚，已合法的 JS 语义不变。
    ///
    /// # Errors
    ///
    /// - 未连接（先 `browse up` 或片段先 connect）。
    /// - 页内抛错（错误带 exceptionDetails 与调试 CTA）。
    /// - 结果不可 JSON 序列化（如 DOM 节点；错误带取原语的 CTA，
    ///   不静默空——undefined 回 null）。
    /// - CDP 调用失败。
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # // no_run：需要已连接引擎的活动 session
    /// # async fn demo(host: &browse_core::JsHost) -> anyhow::Result<()> {
    /// let v = host.eval_js("(() => { const f = s => s.toUpperCase(); return f('ok'); })()").await;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn eval_js(&self, js: &str) -> Result<Value> {
        // 先原样发（表达式形与合法 JS 语义不变）；顶层 return 形在 V8 非法
        // （方言习惯：声明几条加末尾 return），SyntaxError 认出后包
        // async IIFE 重发一次——async 修 await 合法性，换行定界防片段的
        // 行尾注释吞掉闭合括号（评审三洞全收）
        let r = self.evaluate_js_once(js).await?;
        if exception_text(&r).is_some_and(|t| t.contains("Illegal return statement")) {
            let wrapped = format!("(async () => {{\n{js}\n}})()");
            return self.finish_js(&self.evaluate_js_once(&wrapped).await?);
        }
        self.finish_js(&r)
    }

    /// [`Self::eval_js`] 的单发腿：直发 `Runtime.evaluate`，返回 CDP 原始
    /// 结果（exceptionDetails 与值提取由 [`Self::finish_js`] 收口）。
    async fn evaluate_js_once(&self, js: &str) -> Result<Value> {
        self.session
            .call(
                "Runtime.evaluate",
                json!({ "expression": js, "returnByValue": true, "awaitPromise": true }),
            )
            .await
            .map_err(|e| anyhow!("Runtime.evaluate 失败：{e:#}"))
    }

    /// [`Self::eval_js`] 的收口腿：页内抛错带描述与 CTA；值缺失（undefined）
    /// 回 null。边界（评审 G 两层实弹定谳）：`returnByValue` 下 Chrome 把
    /// 不可序列化值（DOM 节点、Date、Map、RegExp、容器内元素）一律序列化
    /// 成 `value:{}` 且不带 subtype——与真空对象协议层不可区分，不硬造
    /// 判据，按空容器口径渲染（CLI 零输出），CTA 写在 surface 的 js-flag
    /// 条目（JS 里自己 JSON.stringify 或取原语）。
    fn finish_js(&self, r: &Value) -> Result<Value> {
        if let Some(text) = exception_text(r) {
            bail!(
                "页内抛错：{text}；下一步：改用 session.waitJs 轮询等条件，或先 console.log 打中间值定位"
            );
        }
        Ok(r.pointer("/result/value").cloned().unwrap_or(Value::Null))
    }

    async fn eval_expr(&self, e: &Expr) -> Result<Value> {
        self.eval_expr_boxed(e).await
    }

    /// 递归经 `BoxFuture` 断链（async fn 不允许直接自递归，E0733）。
    fn eval_expr_boxed<'a>(&'a self, e: &'a Expr) -> futures::future::BoxFuture<'a, Result<Value>> {
        Box::pin(async move {
            // host 侧 await 是透明的：剥掉嵌套 await 再求值
            let e = {
                let mut cur = e;
                while let Expr::Await(inner) = cur {
                    cur = inner;
                }
                cur
            };
            match e {
                // 剥壳循环已消化所有 Await；此臂只为穷尽性存在
                Expr::Await(_) => unreachable!("await 已在上面的剥壳循环消化"),
                Expr::Lit(v) => Ok(v.clone()),
                Expr::Ident(name) => {
                    if name == "session" {
                        return Ok(json!({"__host": "session"}));
                    }
                    if name == "JSON" {
                        return Ok(json!({"__host": "json"}));
                    }
                    if name == "secrets" {
                        return Ok(json!({"__host": "secrets"}));
                    }
                    if name == "undefined" {
                        return Ok(Value::Null);
                    }
                    self.vars
                        .lock()
                        .await
                        .get(name)
                        .cloned()
                        .ok_or_else(|| anyhow!(
                            "未定义变量 {name}；下一步：先在前一条片段里 const {name} = <值>（变量跨调用持久），或检查拼写"
                        ))
                }
                Expr::Object(kvs) => {
                    let mut m = Map::new();
                    for (k, v) in kvs {
                        m.insert(k.clone(), self.eval_expr(v).await?);
                    }
                    Ok(Value::Object(m))
                }
                Expr::Array(xs) => {
                    let mut out = Vec::new();
                    for x in xs {
                        out.push(self.eval_expr(x).await?);
                    }
                    Ok(Value::Array(out))
                }
                Expr::Member { obj, prop } => {
                    let o = self.eval_expr(obj).await?;
                    if o.get("__host").and_then(|v| v.as_str()) == Some("session") {
                        return Ok(json!({"__host": "session", "__domain": prop}));
                    }
                    // 密钥命名空间（#25.4）：secrets.<NAME> 取值；键不存在报 CTA
                    if o.get("__host").and_then(|v| v.as_str()) == Some("secrets") {
                        let Ok(secrets) = SECRETS.lock() else {
                            return Ok(Value::Null);
                        };
                        return secrets
                            .iter()
                            .find(|(k, _)| k == prop)
                            .map(|(_, v)| json!(v.clone()))
                            .ok_or_else(|| anyhow!(
                                "secrets.{prop} 不在已加载密钥里；下一步：检查 --secrets 文件是否含该键或键名拼写"
                            ));
                    }
                    // 内建 length：数组元素数与字符串长度（UTF-16 单元，同 JS）；
                    // serde_json 的 get 只走对象键，字符串与数组在此显式补齐（#15）
                    if prop == "length" {
                        match &o {
                            Value::Array(a) => return Ok(json!(a.len())),
                            Value::String(s) => return Ok(json!(s.encode_utf16().count())),
                            _ => {}
                        }
                    }
                    Ok(o.get(prop).cloned().unwrap_or(Value::Null))
                }
                Expr::Index { obj, index } => {
                    let o = self.eval_expr(obj).await?;
                    let i = self.eval_expr(index).await?;
                    match i {
                        Value::Number(n) => {
                            let idx = n.as_u64().unwrap_or(0) as usize;
                            Ok(o.get(idx).cloned().unwrap_or(Value::Null))
                        }
                        Value::String(s) => Ok(o.get(&s).cloned().unwrap_or(Value::Null)),
                        _ => Ok(Value::Null),
                    }
                }
                Expr::Call { callee, args } => {
                    let mut argv = Vec::new();
                    for a in args {
                        argv.push(self.eval_expr(a).await?);
                    }
                    self.eval_call(callee, &argv).await
                }
            }
        })
    }

    async fn eval_call(&self, callee: &Expr, argv: &[Value]) -> Result<Value> {
        let callee = {
            let mut cur = callee;
            while let Expr::Await(inner) = cur {
                cur = inner;
            }
            cur
        };
        match callee {
            Expr::Ident(name) => self.call_global(name, argv).await,
            Expr::Member { obj, prop } => {
                let o = self.eval_expr(obj).await?;
                if o.get("__host").and_then(|v| v.as_str()) == Some("session") {
                    if let Some(domain) = o.get("__domain").and_then(|v| v.as_str()) {
                        let method = format!("{domain}.{prop}");
                        let params = argv.first().cloned().unwrap_or(json!({}));
                        return self.session.call(&method, params).await;
                    }
                    return self.call_session(prop, argv).await;
                }
                if o.get("__host").and_then(|v| v.as_str()) == Some("json") {
                    return call_json_ns(prop, argv);
                }
                // 值方法面（#21）：字符串与数组的小加工，纯函数无控制流；
                // 不是该方法面的成员落到类型感知 CTA
                if let Some(res) = value_method(&o, prop, argv) {
                    return res;
                }
                if o.is_string() {
                    bail!(
                        "不能调用 {}.{prop}；下一步：字符串可调 {}，序列化走 JSON.stringify，页面内逻辑走 session.Runtime.evaluate",
                        preview(&o),
                        STRING_METHODS.join("/")
                    );
                }
                if o.is_array() {
                    bail!(
                        "不能调用 {}.{prop}；下一步：数组可调 {}，元素级加工走页面侧 session.Runtime.evaluate",
                        preview(&o),
                        ARRAY_METHODS.join("/")
                    );
                }
                bail!(
                    "不能调用 {}.{}；下一步：可调用的是宿主全局函数、值方法面（字符串与数组）或 session.<Domain>.<method>(params)",
                    preview(&o),
                    prop
                )
            }
            _ => bail!("非法调用"),
        }
    }

    async fn call_global(&self, name: &str, argv: &[Value]) -> Result<Value> {
        match name {
            "listPageTargets" => {
                let tabs = self.session.list_page_targets().await?;
                Ok(Value::Array(tabs.into_iter().map(tab_json).collect()))
            }
            "resolveWsUrl" => {
                let opts = connect_opts(argv.first());
                let u = cdp::resolve_ws_url(&opts).await?;
                Ok(json!(u))
            }
            "detectBrowsers" => {
                let hits = cdp::discovery::detect_browsers();
                Ok(Value::Array(
                    hits.into_iter()
                        .map(|b| {
                            json!({
                                "profileDir": b.profile_dir.display().to_string(),
                                "port": b.port,
                                "wsUrl": b.ws_url,
                                "mtimeMs": b.mtime_ms as u64,
                            })
                        })
                        .collect(),
                ))
            }
            // 运行时方法探针（对齐 bh 的 Object.keys(session.Network)）
            "cdpMethods" => {
                let dom = argv.first().and_then(Value::as_str);
                let list = match dom {
                    Some(d) => cdp::methods::methods_of_domain(d),
                    None => METHODS_RAW.lines().collect(),
                };
                Ok(Value::Array(list.into_iter().map(|m| json!(m)).collect()))
            }
            // 本 CLI 的命令面目录探针（与 schema/llms 同源，
            // incur --llms 的运行时等价物）
            "hostFunctions" => Ok(crate::surface::catalog_json()),
            // ---- Chromium 版本管理器（ADR-0007；两源：本地导入 + R2 镜像下载）----
            "chromeInstall" => {
                let opts = argv.first().cloned().unwrap_or(json!({}));
                let from_dir = opts
                    .get("fromDir")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let root = crate::chrome_mgr::chromium_root();
                let brief = match from_dir {
                    Some(from) => {
                        let from = std::path::PathBuf::from(from);
                        let version = opts
                            .get("version")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                            .unwrap_or_else(|| crate::chrome_mgr::version_from_dir_name(&from));
                        tokio::task::spawn_blocking(move || {
                            crate::chrome_mgr::install_from_dir(&root, &version, &from)
                        })
                        .await
                        .map_err(|e| anyhow::anyhow!("安装任务崩了：{e}"))??
                    }
                    // fromDir 缺省走 R2 镜像下载腿（版本发现来源未定标，version 必须显式）
                    None => {
                        let Some(version) = opts
                            .get("version")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                        else {
                            bail!(
                                "镜像下载要显式 version（子域无 manifest 面，版本发现来源\
                                 未定标）；下一步：chromeInstall({{version: \"152.0.7977.84\"}}) \
                                 或带 fromDir 走本地导入"
                            );
                        };
                        tokio::task::spawn_blocking(move || {
                            crate::chrome_mgr::install_from_mirror(&root, &version)
                        })
                        .await
                        .map_err(|e| anyhow::anyhow!("下载任务崩了：{e}"))??
                    }
                };
                Ok(brief)
            }
            "chromeList" => Ok(crate::chrome_mgr::list_json(
                &crate::chrome_mgr::chromium_root(),
            )),
            "chromeUse" => {
                let Some(v) = argv.first().and_then(Value::as_str) else {
                    bail!(
                        "chromeUse 缺 version；下一步：chromeUse(\"152.0.7977.84\")，先 chromeList() 看已装"
                    );
                };
                crate::chrome_mgr::use_version(&crate::chrome_mgr::chromium_root(), v)
            }
            "chromeRemove" => {
                let Some(v) = argv.first().and_then(Value::as_str) else {
                    bail!(
                        "chromeRemove 缺 version；下一步：chromeRemove(\"152.0.7977.84\")，先 chromeList() 看已装"
                    );
                };
                crate::chrome_mgr::remove_version(&crate::chrome_mgr::chromium_root(), v)
            }
            "chromeDoctor" => Ok(crate::chrome_mgr::doctor_json(
                &crate::chrome_mgr::chromium_root(),
            )),
            // 发现源 <mirror>/latest 指针（BROWSE_CHROME_LATEST 可钉）加镜像安装加
            // pin 切换，blocking 全收 spawn_blocking（与 install 腿同规）
            "chromeUpdate" => {
                let root = crate::chrome_mgr::chromium_root();
                tokio::task::spawn_blocking(move || crate::chrome_mgr::update(&root))
                    .await
                    .map_err(|e| anyhow::anyhow!("升级任务崩了：{e}"))?
            }
            // 页内截图存文件，回 {path, bytes}；full 走 captureBeyondViewport
            "screenshot" => {
                // 元素级（#41）：opts.ref 给定则 clip 到该元素 bbox（先滚动
                // 可见再量 rect，口径同 clickRef 的量中心）
                let opts_pre = argv
                    .iter()
                    .find(|v| v.is_object())
                    .cloned()
                    .unwrap_or(json!({}));
                let elem_rect = if let Some(rf) = opts_pre.get("ref").and_then(Value::as_str) {
                    let (bn, owner) = self.lookup_ref_owner(rf).await?;
                    let object_id = crate::semantic::resolve_node_object_in(
                        &self.session,
                        owner.as_deref(),
                        bn,
                    )
                    .await?;
                    let rect = crate::semantic::element_rect_in(
                        &self.session,
                        &object_id,
                        owner.as_deref(),
                    )
                    .await?;
                    if let (Some((x, y, w, h)), Some(child)) = (rect, owner.as_deref()) {
                        let (ox, oy) = crate::semantic::oopif_offset(&self.session, child).await?;
                        Some((x + ox, y + oy, w, h))
                    } else {
                        rect
                    }
                } else {
                    None
                };
                let path = argv.first().and_then(Value::as_str).map(str::to_string);
                let full = argv.get(1).and_then(Value::as_bool).unwrap_or(false);
                // 选项对象（#24）：format jpeg/png、quality（jpeg）、ifChanged
                // （与既有文件逐字节相同即跳过，回 skipped）
                let opts = argv.get(2).cloned().unwrap_or(json!({}));
                let format = opts
                    .get("format")
                    .and_then(Value::as_str)
                    .unwrap_or("png")
                    .to_string();
                if format != "png" && format != "jpeg" {
                    bail!("screenshot 的 format 只支持 png 或 jpeg（当前 {format}）");
                }
                let quality = opts.get("quality").and_then(Value::as_u64).unwrap_or(80);
                let mut params = json!({ "format": format });
                if full {
                    params["captureBeyondViewport"] = json!(true);
                }
                // #41：元素级 clip 与 hires（deviceScaleFactor 全采样）
                if let Some((x, y, w, h)) = elem_rect {
                    params["clip"] = json!({
                        "x": x, "y": y, "width": w, "height": h, "scale": 1.0
                    });
                }
                if opts.get("hires").and_then(Value::as_bool).unwrap_or(false)
                    && let Some(scale) = opts.get("scale").and_then(Value::as_f64)
                {
                    params["clip"]["scale"] = json!(scale.max(1.0));
                }
                if format == "jpeg" {
                    params["quality"] = json!(quality.clamp(1, 100));
                }
                // #34 提交窗竞速走共用重试（评审 G1 helper 化）
                let r = self
                    .call_commit_retry("Page.captureScreenshot", params)
                    .await?;
                let data = r
                    .get("data")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow!("captureScreenshot 未回 data"))?;
                let bytes = base64_decode(data)?;
                let ext = if format == "jpeg" { "jpg" } else { "png" };
                let path = match path {
                    Some(p) => std::path::PathBuf::from(p),
                    None => {
                        let dir = crate::paths::state_dir().join("screenshots");
                        tokio::fs::create_dir_all(&dir).await.ok();
                        let ts = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis())
                            .unwrap_or(0);
                        dir.join(format!("shot-{ts}.{ext}"))
                    }
                };
                // 显式路径无扩展名时补格式后缀（#24）
                let mut path = path;
                if path.extension().is_none() {
                    path.set_extension(ext);
                }
                // ifChanged（#24）：与既有文件逐字节相同即跳过（PNG/JPEG
                // 对同一像素画面确定性编码，字节相同即画面相同；省 token
                // 的读回与再加工）
                if opts
                    .get("ifChanged")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                    && let Ok(prev) = tokio::fs::read(&path).await
                    && prev == bytes
                {
                    return Ok(json!({
                        "path": path.display().to_string(),
                        "bytes": prev.len(),
                        "skipped": true,
                    }));
                }
                let display = path.display().to_string();
                tokio::fs::write(&path, &bytes).await?;
                Ok(json!({ "path": display, "bytes": bytes.len(), "skipped": false }))
            }
            // AX 树快照（对齐 browser-use-pi 的 snapshot 原语）：
            // getFullAXTree -> 精简节点表；url/title 一并带回。
            // 元信息与代际全走 daemon 侧（#30 消注入痕）：url/title 走
            // Target.getTargetInfo（browser 级查询，不触页面求值），代取
            // 文档代计数，页面窗口零写入
            "snapshot" => {
                // #36 捕获与检索分离的捕获面：opts {ref 单元素子树, depth
                // 限深}；childIds 随卷输出供 agent 自行展开
                let opts = argv
                    .first()
                    .filter(|v| v.is_object())
                    .cloned()
                    .unwrap_or(json!({}));
                let (mut nodes, parent) = self.ax_projected().await?;
                // pierce（#47）：同源 iframe 与 shadow DOM 内容不在顶层 AX
                // 树（实弹），DOM.getDocument depth -1 pierce true 穿透走
                // contentDocument 加 shadowRoots；节点带 backendNodeId 可
                // 直接进 ref 表（clickRef/fillRef 跨 frame 生效）。OOPIF
                // 的跨域隔离不吃 pierce（需 auto-attach 子 session），v1
                // 不覆盖已在 surface 披露
                if opts.get("pierce").and_then(Value::as_bool).unwrap_or(false) {
                    let extra = self.pierce_dom_nodes().await?;
                    if !extra.is_empty() {
                        nodes.extend(extra);
                    }
                    // #60 OOPIF：跨域 iframe 走子 session AX 树（DOM
                    // pierce 不含跨域 contentDocument）；先同步一次子 session
                    // 表（本机 Chrome 的 setAutoAttach 不产 iframe 型事件，
                    // 只能扫 Target.getTargets 显式 attach）
                    let _ = self.session.sync_child_sessions().await;
                    let children = self.session.child_sessions().await;
                    for (tid, sid) in children {
                        if let Ok(r) = self
                            .session
                            .call_on("Accessibility.getFullAXTree", json!({}), &sid)
                            .await
                            && let Some(arr) = r.get("nodes").and_then(Value::as_array)
                        {
                            for n in arr {
                                let role = n
                                    .pointer("/role/value")
                                    .and_then(Value::as_str)
                                    .unwrap_or("");
                                if matches!(
                                    role,
                                    "generic"
                                        | "InlineTextBox"
                                        | "presentation"
                                        | "none"
                                        | "RootWebArea"
                                ) {
                                    continue;
                                }
                                let name = n
                                    .pointer("/name/value")
                                    .and_then(Value::as_str)
                                    .unwrap_or("");
                                if name.is_empty() && n.get("backendDOMNodeId").is_none() {
                                    continue;
                                }
                                if let Some(bn) = n.get("backendDOMNodeId").and_then(Value::as_i64)
                                {
                                    nodes.push(json!({
                                        "id": n.get("nodeId"),
                                        "role": role,
                                        "name": name,
                                        "frameId": tid,
                                        "ownerSession": sid,
                                        "value": n.get("value").and_then(|v| v.get("value")),
                                        "backendNodeId": bn,
                                        "oopif": true,
                                    }));
                                }
                            }
                        }
                    }
                }
                // 子树过滤（ref）：命中节点的可见子树
                if let Some(rf) = opts.get("ref").and_then(Value::as_str) {
                    let bn = self.lookup_ref(rf).await?;
                    let root_id = nodes
                        .iter()
                        .find(|n| n.get("backendNodeId") == Some(&json!(bn)))
                        .and_then(|n| n.get("id"))
                        .and_then(Value::as_i64)
                        .ok_or_else(|| {
                            anyhow!(
                                "ref {rf} 在当前 AX 树无对应节点；下一步：重新 snapshot() 取新 ref"
                            )
                        })?;
                    let mut keep = std::collections::HashSet::new();
                    let mut q = vec![root_id];
                    while let Some(cur) = q.pop() {
                        if keep.insert(cur) {
                            for n in nodes.iter().filter(|n| n.get("id") == Some(&json!(cur))) {
                                for c in n
                                    .get("childIds")
                                    .and_then(Value::as_array)
                                    .cloned()
                                    .unwrap_or_default()
                                    .iter()
                                    .filter_map(ax_id)
                                {
                                    q.push(c);
                                }
                            }
                        }
                    }
                    nodes.retain(|n| {
                        n.get("id")
                            .and_then(Value::as_i64)
                            .is_some_and(|id| keep.contains(&id))
                    });
                }
                // 限深（depth）：可见树根起计层，保留前 depth 层
                if let Some(d) = opts.get("depth").and_then(Value::as_u64) {
                    let d = d.max(1) as usize;
                    let ids: std::collections::HashSet<i64> = nodes
                        .iter()
                        .filter_map(|n| n.get("id").and_then(Value::as_i64))
                        .collect();
                    let mut children: HashMap<i64, Vec<i64>> = HashMap::new();
                    for n in &nodes {
                        if let Some(id) = n.get("id").and_then(Value::as_i64) {
                            for c in n
                                .get("childIds")
                                .and_then(Value::as_array)
                                .cloned()
                                .unwrap_or_default()
                                .iter()
                                .filter_map(ax_id)
                            {
                                if ids.contains(&c) {
                                    children.entry(id).or_default().push(c);
                                }
                            }
                        }
                    }
                    let mut depth: HashMap<i64, usize> = HashMap::new();
                    let mut q: Vec<i64> = nodes
                        .iter()
                        .filter_map(|n| n.get("id").and_then(Value::as_i64))
                        .filter(|id| parent.get(id).is_none_or(|p| !ids.contains(p)))
                        .collect();
                    for r in &q {
                        depth.insert(*r, 1);
                    }
                    let mut i = 0;
                    while i < q.len() {
                        let cur = q[i];
                        i += 1;
                        let cd = depth[&cur];
                        if cd >= d {
                            continue;
                        }
                        for c in children.get(&cur).cloned().unwrap_or_default() {
                            if depth.insert(c, cd + 1).is_none() {
                                q.push(c);
                            }
                        }
                    }
                    nodes.retain(|n| {
                        n.get("id")
                            .and_then(Value::as_i64)
                            .is_some_and(|id| depth.get(&id).is_some_and(|dd| *dd <= d))
                    });
                }
                let sid = self.session.get_active_session().await;
                let generation = match &sid {
                    Some(s) => self.session.doc_generation(s).await,
                    None => 0,
                };
                let (url, title) = self.page_meta().await;
                // D35-lite：给带 backendNodeId 的节点依次盖短 ref（e1、e2…），
                // 并整表替换引用表：只有最近一次 snapshot 的 ref 有效
                let mut refmap = HashMap::new();
                let mut counter = 0usize;
                let nodes: Vec<Value> = nodes
                    .into_iter()
                    .filter_map(|mut n| {
                        let bn = n.get("backendNodeId").and_then(Value::as_i64)?;
                        counter += 1;
                        let r = format!("e{counter}");
                        if let Value::Object(m) = &mut n {
                            m.insert("ref".into(), json!(r));
                        }
                        let owner = n
                            .get("ownerSession")
                            .and_then(Value::as_str)
                            .map(str::to_string);
                        refmap.insert(
                            r,
                            RefEntry {
                                backend_node_id: bn,
                                session: owner,
                            },
                        );
                        Some(n)
                    })
                    .collect();
                *self.refs.lock().await = Some(RefTable {
                    session_id: sid.unwrap_or_default(),
                    generation,
                    map: refmap,
                });
                Ok(json!({ "url": url, "title": title, "nodes": nodes }))
            }
            "print" => {
                eprintln!("{}", preview(&argv.first().cloned().unwrap_or(Value::Null)));
                Ok(Value::Null)
            }
            // ---- 语义层近期面（crates/browse-core/src/semantic.rs）----
            "newTab" => {
                crate::semantic::new_tab(&self.session, argv.first().and_then(Value::as_str)).await
            }
            // 一步导航（#19）：navigate 加 waitLoad 一体；opts {timeout 秒,
            // waitIdle: true 或秒数}。已加载页立即返回不阻塞
            "goto" => {
                let url = str_arg(argv, 0, "goto 的 url")?;
                let opts = argv.get(1).cloned().unwrap_or(json!({}));
                let (tmo_ms, warn) = match opts.get("timeout") {
                    None | Some(Value::Null) => secs_to_ms(15),
                    Some(v) => v.as_u64().map(secs_to_ms).ok_or_else(|| {
                        anyhow!("goto 的 timeout 应是整秒数（当前：{}）", preview(v))
                    })?,
                };
                // waitIdle 同 G4 律（评审二轮 G9）：类型不符报错不静默，
                // 数值过 secs_to_ms 拿封顶与溢出防护；其混用告警与
                // timeout 告警合并回传（终审尾巴：别只取毫秒丢告警，
                // 同一混用契约两面一致）
                let mut warn = warn;
                let idle_ms = match opts.get("waitIdle") {
                    None | Some(Value::Null) | Some(Value::Bool(false)) => None,
                    Some(Value::Bool(true)) => Some(20_000),
                    Some(v) => {
                        let (ms, w) = v.as_u64().map(secs_to_ms).ok_or_else(|| {
                            anyhow!(
                                "goto 的 waitIdle 应是 true、秒数或省略（当前：{}）",
                                preview(v)
                            )
                        })?;
                        if let Some(w2) = w {
                            warn = warn.or(Some(w2));
                        }
                        Some(ms)
                    }
                };
                let r = crate::semantic::goto(&self.session, url, tmo_ms, idle_ms).await?;
                Ok(attach_warning(r, warn))
            }
            // 历史导航（#39）：delta 步缺省 1
            "goBack" => {
                let delta = argv.first().and_then(Value::as_u64).unwrap_or(1);
                crate::semantic::go_back(&self.session, delta).await
            }
            "goForward" => {
                let delta = argv.first().and_then(Value::as_u64).unwrap_or(1);
                crate::semantic::go_forward(&self.session, delta).await
            }
            "reload" => {
                let opts = argv.first().cloned().unwrap_or(json!({}));
                let ignore = opts
                    .get("ignoreCache")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                crate::semantic::reload(&self.session, ignore).await
            }
            "switchTab" => {
                let id = argv.first().and_then(Value::as_str).ok_or_else(|| anyhow!(
                    "switchTab 缺 targetId；下一步：switchTab(tabs[0].targetId)，先 const tabs = await listPageTargets()"
                ))?;
                crate::semantic::switch_tab(&self.session, id).await
            }
            "currentTab" => crate::semantic::current_tab(&self.session).await,
            "closeTab" => {
                crate::semantic::close_tab(&self.session, argv.first().and_then(Value::as_str))
                    .await
            }
            "clickAt" => {
                let x = argv.first().and_then(Value::as_i64).ok_or_else(|| anyhow!(
                    "clickAt 缺坐标；下一步：clickAt(x, y)（视口坐标，snapshot+DOM.getBoxModel 量中心）"
                ))?;
                let y = argv.get(1).and_then(Value::as_i64).unwrap_or(0);
                // opts（#35）：button 与 clickCount（2 即双击语义）
                let opts = argv.get(2).cloned().unwrap_or(json!({}));
                let button = opts
                    .get("button")
                    .and_then(Value::as_str)
                    .unwrap_or("left")
                    .to_string();
                let click_count = opts.get("clickCount").and_then(Value::as_i64).unwrap_or(1);
                self.assert_no_dialog().await?;
                crate::semantic::click_at_opts(&self.session, x, y, &button, click_count).await
            }
            "fillInput" => {
                let sel = argv.first().and_then(Value::as_str).ok_or_else(|| anyhow!(
                    "fillInput 缺选择器；下一步：fillInput(\"#q\", \"hello\")（CSS 选择器，填完回读验证）"
                ))?;
                let text = argv.get(1).and_then(Value::as_str).unwrap_or("");
                self.assert_no_dialog().await?;
                let out = crate::semantic::fill_input(&self.session, sel, text).await?;
                // submit（#39）：填完顺带 Enter；返回值保持回读串契约不变
                if submit_arg(argv.get(2)) {
                    crate::semantic::press_key(&self.session, "Enter").await?;
                }
                Ok(out)
            }
            "pressKey" => {
                let key = argv.first().and_then(Value::as_str).ok_or_else(|| anyhow!(
                    "pressKey 缺键名；下一步：pressKey(\"Enter\") / pressKey(\"Tab\") / pressKey(\"a\")"
                ))?;
                self.assert_no_dialog().await?;
                crate::semantic::press_key(&self.session, key).await
            }
            "waitLoad" => {
                // 秒口径（#51）：缺省 10 秒；旧毫秒习惯值由混用守卫换算并告警
                let (ms, warn) = timeout_ms_of(argv, 0, 10, "waitLoad")?;
                let r = crate::semantic::wait_load(&self.session, ms).await?;
                Ok(attach_warning(r, warn))
            }
            "waitIdle" => {
                // 秒口径（#51）：缺省 10 秒
                let (ms, warn) = timeout_ms_of(argv, 0, 10, "waitIdle")?;
                let r = crate::semantic::wait_idle(&self.session, ms).await?;
                Ok(attach_warning(r, warn))
            }
            // ---- 网络响应面（#20）：URL glob 命中等响应完成 ----
            "waitForResponse" => {
                let pat = str_arg(argv, 0, "waitForResponse 的 pattern")?;
                // 秒口径（#51）：缺省 15 秒
                let (ms, warn) = timeout_ms_of(argv, 1, 15, "waitForResponse")?;
                let r = self.wait_for_response(pat, ms).await?;
                Ok(attach_warning(r, warn))
            }
            // ---- 全量 JS 旁路（#22，ADR-0002 修订）：页面逻辑直达 ----
            "pageEval" => {
                let js = str_arg(argv, 0, "pageEval 的 JS 源码")?;
                self.eval_js(js).await
            }
            "responseBody" => {
                let rid = str_arg(argv, 0, "responseBody 的 requestId")?;
                let b = self
                    .session
                    .call("Network.getResponseBody", json!({ "requestId": rid }))
                    .await
                    .map_err(|e| anyhow!(
                        "Network.getResponseBody({rid}) 失败：{e:#}；下一步：requestId 可能已过期释放（No resource with given identifier），findEvents 默认取最早一条，配本函数建议取最新（数组末尾）或直接用 waitForResponse"
                    ))?;
                // 与 waitForResponse 同形（#33 G2）：body/base64Encoded/json 齐
                let decoded = decode_response_body(&b);
                let mut r = json!({
                    "body": decoded["body"].clone(),
                    "base64Encoded": decoded["base64Encoded"].clone(),
                });
                if let Value::String(text) = &decoded["body"]
                    && let Ok(parsed) = serde_json::from_str::<Value>(text)
                {
                    r["json"] = parsed;
                }
                Ok(r)
            }
            // ---- 捕获与检索分离（#36）：服务端搜索，只回命中加祖先链 ----
            "findRefs" => {
                // #36：AX 树在 daemon 侧匹配（name/value 子串，insensitive
                // 开大小写不敏感；不引正则依赖），命中带 context 层祖先链
                // 与 ref（可直接 clickRef），比全量 snapshot 省一个量级
                let q = str_arg(argv, 0, "findRefs 的查询串")?;
                let opts = argv.get(1).cloned().unwrap_or(json!({}));
                let insensitive = opts
                    .get("insensitive")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let context = opts.get("context").and_then(Value::as_u64).unwrap_or(2) as usize;
                let (nodes, parent) = self.ax_projected().await?;
                let needle = if insensitive {
                    q.to_lowercase()
                } else {
                    q.to_string()
                };
                let hit_ids: HashSet<i64> = nodes
                    .iter()
                    .filter_map(|n| {
                        let name = n.get("name").and_then(Value::as_str).unwrap_or("");
                        let val = n.get("value").and_then(Value::as_str).unwrap_or("");
                        let hay = |x: &str| {
                            if insensitive {
                                x.to_lowercase()
                            } else {
                                x.to_string()
                            }
                        };
                        (hay(name).contains(&needle) || hay(val).contains(&needle))
                            .then(|| n.get("id").and_then(Value::as_i64))
                    })
                    .flatten()
                    .collect();
                // 命中加祖先链（context 层）进结果集
                // 零命中早退保留旧引用表（评审 G2）：查一次没查到不该把
                // 手里能用的 ref 全清了；kept_refs 标记语义
                if hit_ids.is_empty() {
                    return Ok(json!({
                        "query": q, "count": 0, "nodes": [], "kept_refs": true,
                    }));
                }
                let mut keep: HashSet<i64> = hit_ids.clone();
                for h in &hit_ids {
                    let mut cur = *h;
                    let mut up = 0;
                    while up < context {
                        if let Some(p) = parent.get(&cur) {
                            keep.insert(*p);
                            cur = *p;
                            up += 1;
                        } else {
                            break;
                        }
                    }
                }
                let sid = self.session.get_active_session().await;
                let generation = match &sid {
                    Some(s) => self.session.doc_generation(s).await,
                    None => 0,
                };
                let (url, title) = self.page_meta().await;
                // ref 只盖结果集（引用表整表替换，同 snapshot 语义）
                let mut refmap = HashMap::new();
                let mut counter = 0usize;
                let kept: Vec<Value> = nodes
                    .iter()
                    .filter(|n| {
                        n.get("id")
                            .and_then(Value::as_i64)
                            .is_some_and(|i| keep.contains(&i))
                    })
                    .filter_map(|n| {
                        let bn = n.get("backendNodeId").and_then(Value::as_i64)?;
                        counter += 1;
                        let r = format!("e{counter}");
                        let hit = n
                            .get("id")
                            .and_then(Value::as_i64)
                            .is_some_and(|i| hit_ids.contains(&i));
                        let owner = n
                            .get("ownerSession")
                            .and_then(Value::as_str)
                            .map(str::to_string);
                        refmap.insert(
                            r.clone(),
                            RefEntry {
                                backend_node_id: bn,
                                session: owner,
                            },
                        );
                        Some(json!({
                            "ref": r, "hit": hit,
                            "id": n.get("id"), "role": n.get("role"),
                            "frameId": n.get("frameId"),
                            "name": n.get("name"), "value": n.get("value"),
                            "checked": n.get("checked"), "disabled": n.get("disabled"),
                            "backendNodeId": bn,
                        }))
                    })
                    .collect();
                let count = kept
                    .iter()
                    .filter(|n| n.get("hit") == Some(&json!(true)))
                    .count();
                *self.refs.lock().await = Some(RefTable {
                    session_id: sid.unwrap_or_default(),
                    generation,
                    map: refmap,
                });
                Ok(json!({ "url": url, "title": title, "query": q, "count": count, "nodes": kept }))
            }
            // ---- 页面可观测性三件（#37）----
            "console" => {
                // #37：控制台分级检索。Runtime 域随 tab 入口自动开（开域前
                // 的旧消息收不到）；缓冲环形 1000 条，超量挤老
                let _ = self.session.call("Runtime.enable", json!({})).await;
                let opts = argv.first().cloned().unwrap_or(json!({}));
                let since = opts.get("since").and_then(Value::as_u64).unwrap_or(0);
                let min_level = opts
                    .get("minLevel")
                    .and_then(Value::as_str)
                    .unwrap_or("verbose");
                let rank = |l: &str| match l {
                    "error" | "assert" => 0,
                    "warning" => 1,
                    "log" | "info" => 2,
                    "debug" => 3,
                    _ => 4,
                };
                let min_rank = rank(min_level);
                let active = self.session.get_active_session().await;
                let evs = self
                    .session
                    .peek_events_since("Runtime.consoleAPICalled", since, 500)
                    .await;
                let evs: Vec<Value> = evs
                    .into_iter()
                    .filter(|e| from_active_session(&active, e))
                    .collect();
                let rows: Vec<Value> = evs
                    .iter()
                    .filter_map(|e| {
                        let p = &e["params"];
                        let level = p.get("type").and_then(Value::as_str).unwrap_or("log");
                        if rank(level) > min_rank {
                            return None;
                        }
                        let text = p
                            .get("args")
                            .and_then(Value::as_array)
                            .map(|a| {
                                a.iter()
                                    .filter_map(|v| v.get("value"))
                                    .map(|v| {
                                        v.as_str()
                                            .map(str::to_string)
                                            .unwrap_or_else(|| v.to_string())
                                    })
                                    .collect::<Vec<_>>()
                                    .join(" ")
                            })
                            .unwrap_or_default();
                        Some(json!({ "seq": e.get("seq"), "level": level, "text": text }))
                    })
                    .collect();
                Ok(json!({ "count": rows.len(), "messages": rows }))
            }
            "jsErrors" => {
                // #37：未捕获异常（exceptionThrown 本就是未捕获面）
                let _ = self.session.call("Runtime.enable", json!({})).await;
                let since = argv
                    .first()
                    .and_then(|v| v.get("since"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let active = self.session.get_active_session().await;
                let evs = self
                    .session
                    .peek_events_since("Runtime.exceptionThrown", since, 200)
                    .await;
                let evs: Vec<Value> = evs
                    .into_iter()
                    .filter(|e| from_active_session(&active, e))
                    .collect();
                let rows: Vec<Value> = evs
                    .iter()
                    .map(|e| {
                        let x = &e["params"]["exceptionDetails"];
                        let desc = x
                            .pointer("/exception/description")
                            .and_then(Value::as_str)
                            .unwrap_or_else(|| x.get("text").and_then(Value::as_str).unwrap_or(""));
                        json!({
                            "seq": e.get("seq"), "text": desc,
                            "url": x.get("url"), "line": x.get("lineNumber"),
                        })
                    })
                    .collect();
                Ok(json!({ "count": rows.len(), "errors": rows }))
            }
            "requests" => {
                // #37：网络响应摘要列表（Network 域随 tab 入口自动开）；
                // filter 是 url 子串。环形 1000 条，超量挤老
                let opts = argv.first().cloned().unwrap_or(json!({}));
                let since = opts.get("since").and_then(Value::as_u64).unwrap_or(0);
                let filter = opts.get("filter").and_then(Value::as_str).unwrap_or("");
                let active = self.session.get_active_session().await;
                let evs = self
                    .session
                    .peek_events_since("Network.responseReceived", since, 1000)
                    .await;
                let evs: Vec<Value> = evs
                    .into_iter()
                    .filter(|e| from_active_session(&active, e))
                    .collect();
                let mut rows: Vec<Value> = Vec::new();
                for e in &evs {
                    let p = &e["params"];
                    let resp = &p["response"];
                    let url = resp.get("url").and_then(Value::as_str).unwrap_or("");
                    if !filter.is_empty() && !url.contains(filter) {
                        continue;
                    }
                    rows.push(json!({
                        "index": rows.len(),
                        "requestId": p.get("requestId"),
                        "url": url,
                        "status": resp.get("status"),
                        "type": p.get("type"),
                        "bytes": resp.get("encodedDataLength"),
                    }));
                }
                Ok(json!({ "count": rows.len(), "requests": rows }))
            }
            "requestDetail" => {
                // #37：单条详情。index 是 since 窗内（未过滤）序号；也可直
                // 接给 requestId（取最新一条）。body 另走 responseBody
                let opts = argv.get(1).cloned().unwrap_or(json!({}));
                let since = opts.get("since").and_then(Value::as_u64).unwrap_or(0);
                // filter 与 requests() 同解析（评审 F2）：index 语义随之一致
                let filter = opts.get("filter").and_then(Value::as_str).unwrap_or("");
                let active = self.session.get_active_session().await;
                let evs = self
                    .session
                    .peek_events_since("Network.responseReceived", since, 1000)
                    .await;
                let evs: Vec<Value> = evs
                    .into_iter()
                    .filter(|e| {
                        from_active_session(&active, e)
                            && (filter.is_empty()
                                || e.pointer("/params/response/url")
                                    .and_then(Value::as_str)
                                    .is_some_and(|u| u.contains(filter)))
                    })
                    .collect();
                let hit = match argv.first() {
                    Some(Value::Number(n)) => n
                        .as_u64()
                        .and_then(|i| evs.get(i as usize).cloned()),
                    Some(v) => v.as_str().and_then(|rid| {
                        evs.iter()
                            .rev()
                            .find(|e| e["params"]["requestId"] == json!(rid))
                            .cloned()
                    }),
                    _ => None,
                }
                .ok_or_else(|| anyhow!(
                    "requestDetail 没找到目标（index 越界或 requestId 不在缓冲）；下一步：先 requests() 看列表（环形 1000 条，老事件可能被挤掉）"
                ))?;
                let p = &hit["params"];
                let resp = &p["response"];
                Ok(json!({
                    "requestId": p.get("requestId"),
                    "url": resp.get("url"),
                    "status": resp.get("status"),
                    "type": p.get("type"),
                    "mimeType": resp.get("mimeType"),
                    "headers": resp.get("headers"),
                    "bytes": resp.get("encodedDataLength"),
                    "bodyHint": "body 走 responseBody(requestId)",
                }))
            }
            // ---- 鼠标原语族（#35）----
            "mouseMove" => {
                let x = argv
                    .first()
                    .and_then(Value::as_i64)
                    .ok_or_else(|| anyhow!("mouseMove 缺坐标；下一步：mouseMove(x, y)"))?;
                let y = argv.get(1).and_then(Value::as_i64).unwrap_or(0);
                self.assert_no_dialog().await?;
                crate::semantic::mouse_move(&self.session, x, y).await
            }
            "mouseDown" => {
                // button? x? y?：坐标缺省沿用最近 mouseMove 落点（评审 F1）
                let b = argv
                    .first()
                    .and_then(Value::as_str)
                    .unwrap_or("left")
                    .to_string();
                let x = argv.get(1).and_then(Value::as_i64);
                let y = argv.get(2).and_then(Value::as_i64);
                self.assert_no_dialog().await?;
                crate::semantic::mouse_down(&self.session, x, y, &b).await
            }
            "mouseUp" => {
                let b = argv
                    .first()
                    .and_then(Value::as_str)
                    .unwrap_or("left")
                    .to_string();
                let x = argv.get(1).and_then(Value::as_i64);
                let y = argv.get(2).and_then(Value::as_i64);
                self.assert_no_dialog().await?;
                crate::semantic::mouse_up(&self.session, x, y, &b).await
            }
            "mouseWheel" => {
                let dx = argv.first().and_then(Value::as_i64).unwrap_or(0);
                let dy = argv.get(1).and_then(Value::as_i64).unwrap_or(0);
                self.assert_no_dialog().await?;
                crate::semantic::mouse_wheel(&self.session, dx, dy).await
            }
            // 文件拖放（#35）：DOM.setFileInputFiles 直灌（可靠面），拖拽
            // 事件序列留给 mouseDown/Move/Up 原语手拼
            "dropFiles" => {
                let r = argv.first().and_then(Value::as_str).ok_or_else(|| anyhow!(
                    "dropFiles 缺 ref；下一步：dropFiles(\"e3\", [\"/abs/a.png\", \"/abs/b.txt\"])"
                ))?;
                let files = argv
                    .get(1)
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                if files.is_empty() {
                    bail!("dropFiles 缺文件路径数组；下一步：第二参给绝对路径数组");
                }
                // 路径预检（#35 评审 F1b）：CDP 不校验路径，坏路径静默假成功
                let missing: Vec<String> = files
                    .iter()
                    .filter_map(Value::as_str)
                    .filter(|p| !std::path::Path::new(p).is_file())
                    .map(str::to_string)
                    .collect();
                if !missing.is_empty() {
                    bail!(
                        "dropFiles 这些路径在 daemon 侧不存在（CDP 不校验会假成功）：{}；下一步：给存在的绝对路径",
                        missing.join(", ")
                    );
                }
                let (bn, owner) = self.lookup_ref_owner(r).await?;
                match owner.as_deref() {
                    Some(sid) => {
                        self.session
                            .call_on(
                                "DOM.setFileInputFiles",
                                json!({ "files": files, "backendNodeId": bn }),
                                sid,
                            )
                            .await?
                    }
                    None => {
                        self.session
                            .call(
                                "DOM.setFileInputFiles",
                                json!({ "files": files, "backendNodeId": bn }),
                            )
                            .await?
                    }
                };
                Ok(json!(true))
            }
            // ---- 下载捕获（#38）----
            "downloads" => {
                // Browser 域事件（无 sessionId），不做活动 tab 过滤；
                // 行情自 downloadWillBegin，进度取同 guid 的最新
                // downloadProgress
                let since = argv
                    .first()
                    .and_then(|v| v.get("since"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let begins = self
                    .session
                    .peek_events_since("Browser.downloadWillBegin", since, 200)
                    .await;
                let progresses = self
                    .session
                    .peek_events_since("Browser.downloadProgress", 0, 1000)
                    .await;
                let rows: Vec<Value> = begins
                    .iter()
                    .map(|b| {
                        let guid = b.pointer("/params/guid").cloned().unwrap_or(Value::Null);
                        let latest = progresses
                            .iter()
                            .rev()
                            .find(|p| p.pointer("/params/guid") == Some(&guid));
                        json!({
                            "guid": guid,
                            "url": b.pointer("/params/url"),
                            "filename": b.pointer("/params/suggestedFilename"),
                            "state": latest
                                .and_then(|p| p.pointer("/params/state"))
                                .cloned()
                                .unwrap_or(json!("inProgress")),
                            "receivedBytes": latest
                                .and_then(|p| p.pointer("/params/receivedBytes"))
                                .cloned()
                                .unwrap_or(json!(0)),
                            "totalBytes": latest
                                .and_then(|p| p.pointer("/params/totalBytes"))
                                .cloned()
                                .unwrap_or(json!(0)),
                        })
                    })
                    .collect();
                Ok(json!({ "count": rows.len(), "downloads": rows }))
            }
            "downloadPath" => {
                // 等 completed 后给落盘路径（drops/downloads 目录）；至多
                // 20 秒，超时报当前态与已知路径候选
                let guid = str_arg(argv, 0, "downloadPath 的 guid")?;
                let timeout_s = argv.get(1).and_then(Value::as_u64).unwrap_or(20);
                let deadline =
                    tokio::time::Instant::now() + std::time::Duration::from_secs(timeout_s);
                let dir = crate::paths::state_dir().join("downloads");
                loop {
                    let progresses = self
                        .session
                        .peek_events_since("Browser.downloadProgress", 0, 1000)
                        .await;
                    let done = progresses
                        .iter()
                        .rev()
                        .find(|p| p.pointer("/params/guid") == Some(&json!(guid)));
                    let state = done
                        .and_then(|p| p.pointer("/params/state"))
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let begins = self
                        .session
                        .peek_events_since("Browser.downloadWillBegin", 0, 200)
                        .await;
                    let suggested = begins
                        .iter()
                        .find(|b| b.pointer("/params/guid") == Some(&json!(guid)))
                        .and_then(|b| b.pointer("/params/suggestedFilename"))
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    if state == "completed" {
                        let named = dir.join(&suggested);
                        if named.is_file() {
                            return Ok(json!({
                                "path": named.display().to_string(),
                                "state": "completed",
                            }));
                        }
                        // 完成但改名未落：给 guid 原始名候选
                        return Ok(json!({
                            "path": dir.join(guid).display().to_string(),
                            "named": named.display().to_string(),
                            "state": "completed",
                        }));
                    }
                    if state == "canceled" {
                        bail!("下载 {guid} 已取消");
                    }
                    if tokio::time::Instant::now() >= deadline {
                        bail!(
                            "downloadPath 等待 {timeout_s} 秒未完成（当前态 {state:?}）；下一步：downloads() 看进度，或加大等待秒数"
                        );
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                }
            }
            // ---- 人机协作视觉件（#41）----
            "highlight" => {
                // 持久高亮覆盖层（人看 agent 在操作哪个元素）：label 给定
                // 时叠编号徽标（annotate 形态，与 snapshot ref 对齐由调用
                // 方保证——ref 的序号即 label）
                let r = str_arg(argv, 0, "highlight 的 ref")?;
                let opts = argv.get(1).cloned().unwrap_or(json!({}));
                let label = opts
                    .get("label")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let (bn, owner) = self.lookup_ref_owner(r).await?;
                crate::semantic::highlight_in(&self.session, bn, label.as_deref(), owner.as_deref())
                    .await
            }
            "highlightClear" => {
                self.session
                    .call(
                        "Runtime.evaluate",
                        json!({
                            "expression": "(() => { document.querySelectorAll('[data-browse-hl]').forEach(n => n.remove()); return true })()",
                            "returnByValue": true
                        }),
                    )
                    .await?;
                Ok(json!(true))
            }
            "annotate" => {
                // 截图叠加编号标签（#24 残余）：对一批 ref 画框加编号徽标，
                // 截图后 highlightClear 收场
                let refs = argv
                    .first()
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                if refs.is_empty() {
                    bail!(
                        "annotate 缺 ref 数组；下一步：annotate([\"e1\", \"e3\"])，ref 来自最近一次 snapshot()"
                    );
                }
                let mut drawn = 0usize;
                for (i, rv) in refs.iter().enumerate() {
                    let r = match rv.as_str() {
                        Some(r) => r,
                        None => {
                            // 失败先清场（评审 F2）：不留半批框污染页面
                            self.highlight_clear_quiet().await;
                            bail!(
                                "annotate 的第 {} 项应是 ref 字符串（当前：{}）；已画 {} 项已清场",
                                i + 1,
                                preview(rv),
                                drawn
                            );
                        }
                    };
                    let (bn, owner) = match self.lookup_ref_owner(r).await {
                        Ok(bn) => bn,
                        Err(e) => {
                            self.highlight_clear_quiet().await;
                            bail!(
                                "annotate 第 {} 项 {r} 取节点失败：{e:#}；已画 {} 项已清场；下一步：重新 snapshot() 核对 ref",
                                i + 1,
                                drawn
                            );
                        }
                    };
                    if let Err(e) =
                        crate::semantic::highlight_in(&self.session, bn, Some(r), owner.as_deref())
                            .await
                    {
                        self.highlight_clear_quiet().await;
                        bail!(
                            "annotate 第 {} 项 {r} 画框失败：{e:#}；已画 {} 项已清场",
                            i + 1,
                            drawn
                        );
                    }
                    drawn += 1;
                }
                Ok(json!({ "drawn": drawn }))
            }
            // 开关级权限授予（#28）：浏览器级 grantPermissions
            "grantPermissions" => {
                let perms: Vec<String> = argv
                    .first()
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default();
                if perms.is_empty() {
                    bail!(
                        "grantPermissions 缺权限数组；下一步：grantPermissions([\"geolocation\", \"clipboard-read\"])（CDP 枚举）"
                    );
                }
                let origin = argv.get(1).and_then(Value::as_str);
                crate::semantic::grant_permissions(&self.session, &perms, origin).await
            }
            // 登录态按域克隆（#48）：从附着浏览器只读热迁到当前引擎
            "cloneCookies" => {
                let domains: Vec<String> = argv
                    .first()
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default();
                crate::cookie_clone::clone_domains(&self.session, &domains).await
            }
            // ---- a11y 媒质仿真族（#40）----
            "emulateMedia" => {
                let opts = argv.first().cloned().unwrap_or(json!({}));
                crate::semantic::emulate_media(&self.session, &opts).await
            }
            "emulateMediaClear" => crate::semantic::emulate_media_clear(&self.session).await,
            // ---- storage 颗粒度 CRUD（#42）----
            "cookies" => {
                let domain = argv.first().and_then(Value::as_str);
                crate::semantic::cookies(&self.session, domain).await
            }
            "cookieGet" => {
                let name = str_arg(argv, 0, "cookieGet 的 name")?;
                let all = crate::semantic::cookies(&self.session, None).await?;
                let hit = all
                    .as_array()
                    .and_then(|a| a.iter().find(|c| c.get("name") == Some(&json!(name))))
                    .cloned();
                Ok(hit.unwrap_or(Value::Null))
            }
            "cookieSet" => {
                let name = str_arg(argv, 0, "cookieSet 的 name")?;
                let value = str_arg(argv, 1, "cookieSet 的 value")?;
                let opts = argv.get(2).cloned().unwrap_or(json!({}));
                crate::semantic::cookie_set(&self.session, name, value, &opts).await
            }
            "cookieDelete" => {
                let name = str_arg(argv, 0, "cookieDelete 的 name")?;
                let domain = argv.get(1).and_then(Value::as_str);
                crate::semantic::cookie_delete(&self.session, name, domain).await
            }
            "cookiesClear" => crate::semantic::cookies_clear(&self.session).await,
            "localGet" => {
                let k = str_arg(argv, 0, "localGet 的 key")?;
                crate::semantic::storage_op_pub(&self.session, "localStorage", "get", Some(k), None)
                    .await
            }
            "localSet" => {
                let k = str_arg(argv, 0, "localSet 的 key")?;
                let v = str_arg(argv, 1, "localSet 的 value")?;
                crate::semantic::storage_op_pub(
                    &self.session,
                    "localStorage",
                    "set",
                    Some(k),
                    Some(v),
                )
                .await
            }
            "localDelete" => {
                let k = str_arg(argv, 0, "localDelete 的 key")?;
                crate::semantic::storage_op_pub(
                    &self.session,
                    "localStorage",
                    "remove",
                    Some(k),
                    None,
                )
                .await
            }
            "localClear" => {
                crate::semantic::storage_op_pub(&self.session, "localStorage", "clear", None, None)
                    .await
            }
            "sessionGet" => {
                let k = str_arg(argv, 0, "sessionGet 的 key")?;
                crate::semantic::storage_op_pub(
                    &self.session,
                    "sessionStorage",
                    "get",
                    Some(k),
                    None,
                )
                .await
            }
            "sessionSet" => {
                let k = str_arg(argv, 0, "sessionSet 的 key")?;
                let v = str_arg(argv, 1, "sessionSet 的 value")?;
                crate::semantic::storage_op_pub(
                    &self.session,
                    "sessionStorage",
                    "set",
                    Some(k),
                    Some(v),
                )
                .await
            }
            "sessionDelete" => {
                let k = str_arg(argv, 0, "sessionDelete 的 key")?;
                crate::semantic::storage_op_pub(
                    &self.session,
                    "sessionStorage",
                    "remove",
                    Some(k),
                    None,
                )
                .await
            }
            "sessionClear" => {
                crate::semantic::storage_op_pub(
                    &self.session,
                    "sessionStorage",
                    "clear",
                    None,
                    None,
                )
                .await
            }
            // ---- 页面诊断七判（#49）：结构化判读 {verdict, evidence, suggestion} ----
            "detect" => {
                // 页内探针一次打包（readyState/title/正文长/节点数/密码框），
                // 网络信号取事件缓冲（403/429/503/失败计数）
                let probe = self
                    .session
                    .call(
                        "Runtime.evaluate",
                        json!({
                            "expression": r#"JSON.stringify((() => {
                                const b = document.body;
                                const txt = b ? (b.innerText || '') : '';
                                return {
                                    readyState: document.readyState,
                                    title: document.title || '',
                                    textLen: txt.length,
                                    nodeCount: document.querySelectorAll('*').length,
                                    hasPassword: !!document.querySelector('input[type=password]'),
                                };
                            })())"#,
                            "returnByValue": true
                        }),
                    )
                    .await;
                let p: Value = probe
                    .ok()
                    .and_then(|v| {
                        v.pointer("/result/value")
                            .and_then(Value::as_str)
                            .and_then(|t| serde_json::from_str(t).ok())
                    })
                    .unwrap_or(json!({}));
                let title = p
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_lowercase();
                let ready = p.get("readyState").and_then(Value::as_str).unwrap_or("");
                let text_len = p.get("textLen").and_then(Value::as_u64).unwrap_or(0);
                let node_count = p.get("nodeCount").and_then(Value::as_u64).unwrap_or(0);
                let has_password = p
                    .get("hasPassword")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                // 只认活动 tab 的网络信号（评审 F1）：后台 tab 的 403/429
                // 不劫持本页判读
                let active = self.session.get_active_session().await;
                let statuses = self
                    .session
                    .peek_events_since("Network.responseReceived", 0, 1000)
                    .await;
                let statuses: Vec<Value> = statuses
                    .into_iter()
                    .filter(|e| from_active_session(&active, e))
                    .collect();
                let code = |e: &Value| e.pointer("/params/response/status").and_then(Value::as_u64);
                let c403 = statuses.iter().filter(|e| code(e) == Some(403)).count();
                let c429 = statuses.iter().filter(|e| code(e) == Some(429)).count();
                let c503 = statuses.iter().filter(|e| code(e) == Some(503)).count();
                let failed = self
                    .session
                    .peek_events_since("Network.loadingFailed", 0, 200)
                    .await
                    .into_iter()
                    .filter(|e| from_active_session(&active, e))
                    .count();
                let mut evidence: Vec<String> = Vec::new();
                let title_disp = p.get("title").cloned().unwrap_or(json!(""));
                evidence.push(format!("title={title_disp}"));
                evidence.push(format!("readyState={ready}"));
                evidence.push(format!("bodyTextLen={text_len}, nodeCount={node_count}"));
                if c403 + c429 + c503 + failed > 0 {
                    evidence.push(format!(
                        "network: 403x{c403}, 429x{c429}, 503x{c503}, failedx{failed}"
                    ));
                }
                let challenge_kw = [
                    "just a moment",
                    "attention required",
                    "checking your browser",
                    "verify you are human",
                    "challenge",
                ];
                let challenge_hit = challenge_kw.iter().any(|k| title.contains(k));
                let (verdict, suggestion) = if challenge_hit && (c403 + c503 > 0 || text_len < 600)
                {
                    (
                        "challenged",
                        "人机挑战页：等几秒复测 detect()，或换附着态人工过验证；routeMock 不可绕真挑战",
                    )
                } else if c429 > 0 {
                    (
                        "rate-limited",
                        "资源限流：放缓节奏或稍后重试；requests({filter}) 看命中端点",
                    )
                } else if c403 > 0 {
                    (
                        "blocked",
                        "被拒（403）：换 UA/出口或检查目标权限；requestDetail 看是哪条",
                    )
                } else if failed > 0 && ready != "complete" {
                    (
                        "stalled",
                        "加载停滞：waitLoad(10) 再试，requests() 看卡住的请求",
                    )
                } else if has_password && ready == "complete" {
                    // 密码框在即判登录墙（评审 G1：现代登录页营销文案轻易
                    // 破正文阈值，保守方向宁可误报登录墙不误报 ok）
                    (
                        "login-wall",
                        "登录墙：先补登录态（storageState 或 cookie 注入）再取数据",
                    )
                } else if ready == "complete" && text_len < 15 && node_count < 10 {
                    (
                        "blank",
                        "空白页：等渲染 waitJs(\"document.body.children.length > 1\")，或 jsErrors() 看渲染炸在哪",
                    )
                } else if ready != "complete" {
                    ("loading", "仍在加载：goto/waitLoad 收尾后复测")
                } else {
                    ("ok", "页面正常：snapshot()/findRefs() 取结构")
                };
                Ok(json!({
                    "verdict": verdict,
                    "evidence": evidence,
                    "suggestion": suggestion,
                }))
            }
            // ---- 交互动词补全（#23）与运营面（#25.2/#25.3）----
            "hoverRef" => {
                let r = str_arg(argv, 0, "hoverRef 的 ref")?;
                self.assert_no_dialog().await?;
                let (bn, owner) = self.lookup_ref_owner(r).await?;
                crate::semantic::hover_ref_in(&self.session, bn, owner.as_deref()).await
            }
            "hoverAt" => {
                let x = int_arg(argv, 0, "hoverAt 的 x")?;
                let y = int_arg(argv, 1, "hoverAt 的 y")?;
                self.assert_no_dialog().await?;
                crate::semantic::hover_at(&self.session, x, y).await
            }
            "dblclickRef" => {
                let r = str_arg(argv, 0, "dblclickRef 的 ref")?;
                self.assert_no_dialog().await?;
                let (bn, owner) = self.lookup_ref_owner(r).await?;
                crate::semantic::dblclick_ref_in(&self.session, bn, owner.as_deref()).await
            }
            "dragRef" => {
                let src = str_arg(argv, 0, "dragRef 的源 ref")?;
                let dst = str_arg(argv, 1, "dragRef 的目标 ref")?;
                self.assert_no_dialog().await?;
                let (sb, sowner) = self.lookup_ref_owner(src).await?;
                let (db, downer) = self.lookup_ref_owner(dst).await?;
                crate::semantic::drag_ref_in(
                    &self.session,
                    sb,
                    db,
                    sowner.as_deref(),
                    downer.as_deref(),
                )
                .await
            }
            "keydown" => {
                let k = str_arg(argv, 0, "keydown 的键")?;
                self.assert_no_dialog().await?;
                crate::semantic::key_raw(&self.session, k, true).await
            }
            "keyup" => {
                let k = str_arg(argv, 0, "keyup 的键")?;
                crate::semantic::key_raw(&self.session, k, false).await
            }
            "typeRef" => {
                let r = str_arg(argv, 0, "typeRef 的 ref")?;
                let text = str_arg(argv, 1, "typeRef 的文本")?;
                self.assert_no_dialog().await?;
                let (bn, owner) = self.lookup_ref_owner(r).await?;
                crate::semantic::type_ref_in(&self.session, bn, text, owner.as_deref()).await
            }
            "emulate" => {
                let opts = argv.first().cloned().ok_or_else(|| anyhow!(
                    "emulate 需要 opts 对象；下一步：emulate({{viewport:{{width:390,height:844}}, mobile:true}}) 或 emulate({{userAgent:\"...\"}})"
                ))?;
                crate::semantic::emulate(&self.session, &opts).await
            }
            "setInitScript" => {
                // #25.2：存共享态（watcher 对新 session 补注）+ 立即对活动
                // session 生效。替换/清除先撤旧注册（评审 F1）：注册跨
                // reload 存活，不撤会叠加
                let code = str_arg(argv, 0, "setInitScript 的代码")?;
                let olds: Vec<(String, Vec<String>)> =
                    self.init_script_ids.lock().await.drain().collect();
                for (sid, idents) in olds {
                    for ident in idents {
                        let _ = self
                            .session
                            .call_on(
                                "Page.removeScriptToEvaluateOnNewDocument",
                                json!({ "identifier": ident }),
                                &sid,
                            )
                            .await;
                    }
                }
                let applied = if code.is_empty() {
                    None
                } else {
                    Some(code.to_string())
                };
                if let Ok(mut g) = self.init_script.lock() {
                    *g = applied;
                }
                if let Some(code) = self.init_script.lock().ok().and_then(|g| g.clone())
                    && let Some(sid) = self.session.get_active_session().await
                    && let Ok(r) = self
                        .session
                        .call_on(
                            "Page.addScriptToEvaluateOnNewDocument",
                            json!({ "source": code }),
                            &sid,
                        )
                        .await
                    && let Some(ident) = r
                        .get("identifier")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                {
                    self.init_script_ids
                        .lock()
                        .await
                        .entry(sid)
                        .or_default()
                        .push(ident);
                }
                Ok(json!(true))
            }
            "exportStorageState" => crate::semantic::export_storage_state(&self.session).await,
            "importStorageState" => {
                let arg = argv.first().cloned().ok_or_else(|| anyhow!(
                    "importStorageState 需要 exportStorageState 的返回值或文件路径串；下一步：const st = <导出值> 后 importStorageState(st)"
                ))?;
                crate::semantic::import_storage_state(&self.session, &arg).await
            }
            // ---- 元素引用（D35-lite）：ref 来自最近一次 snapshot() ----
            "clickRef" => {
                let r = argv.first().and_then(Value::as_str).ok_or_else(|| anyhow!(
                    "clickRef 缺 ref；下一步：clickRef(\"e3\")，ref 在最近一次 snapshot() 返回的 nodes[].ref"
                ))?;
                self.assert_no_dialog().await?;
                let (bn, owner) = self.lookup_ref_owner(r).await?;
                let opts = argv.get(1).cloned().unwrap_or(json!({}));
                // 点击可能触发用户侧导航（无提交屏障背书）：点前记事件水位
                // 供 waitNav 的有界提交等待（评审二轮 F4）
                let nav_since = self.session.last_seq().await;
                let button = opts
                    .get("button")
                    .and_then(Value::as_str)
                    .unwrap_or("left")
                    .to_string();
                let click_count = opts.get("clickCount").and_then(Value::as_i64).unwrap_or(1);
                let mut out = crate::semantic::click_ref_opts_in(
                    &self.session,
                    bn,
                    owner.as_deref(),
                    &button,
                    click_count,
                )
                .await?;
                // waitNav（#19）：链接型点击后自动等导航稳定，免点击加
                // waitLoad 两步；同文档锚点与纯 JS 按钮等已加载页 grace 窗
                // 后返回。clickRef 基础回执是裸 true（布尔面），waitNav 在位
                // 时显式构造对象形 {clicked, waitLoad[, timeoutWarning]}（评审
                // F2：布尔面附不上键，静默丢弃即特性不可见）
                if opts.get("waitNav").and_then(Value::as_bool) == Some(true) {
                    let (ms, warn) = match opts.get("timeout") {
                        None | Some(Value::Null) => secs_to_ms(10),
                        Some(v) => v.as_u64().map(secs_to_ms).ok_or_else(|| {
                            anyhow!(
                                "clickRef waitNav 的 timeout 应是整秒数（当前：{}）",
                                preview(v)
                            )
                        })?,
                    };
                    let wl =
                        crate::semantic::wait_settled(&self.session, nav_since, 2_000.min(ms), ms)
                            .await?;
                    let mut o = serde_json::Map::new();
                    o.insert("clicked".to_string(), out.clone());
                    o.insert("waitLoad".to_string(), wl);
                    if let Some(w) = warn {
                        o.insert("timeoutWarning".to_string(), json!(w));
                    }
                    out = Value::Object(o);
                }
                Ok(out)
            }
            "fillRef" => {
                let r = argv.first().and_then(Value::as_str).ok_or_else(|| anyhow!(
                    "fillRef 缺 ref；下一步：fillRef(\"e2\", \"hello\")，ref 在最近一次 snapshot() 返回的 nodes[].ref"
                ))?;
                let text = argv.get(1).and_then(Value::as_str).unwrap_or("");
                self.assert_no_dialog().await?;
                let (bn, owner) = self.lookup_ref_owner(r).await?;
                let out =
                    crate::semantic::fill_ref_in(&self.session, bn, text, owner.as_deref()).await?;
                // submit（#39）：填完顺带 Enter；返回值保持回读串契约不变，
                // 提交副作用由页内可观察
                if submit_arg(argv.get(2)) {
                    crate::semantic::press_key(&self.session, "Enter").await?;
                }
                Ok(out)
            }
            "selectOption" => {
                let r = argv.first().and_then(Value::as_str).ok_or_else(|| anyhow!(
                    "selectOption 缺 ref；下一步：selectOption(\"e4\", \"Beta\")（value 或可见 label，ref 来自 snapshot()）"
                ))?;
                let value = argv.get(1).and_then(Value::as_str).ok_or_else(|| anyhow!(
                    "selectOption 缺选项值；下一步：selectOption(\"e4\", \"Beta\")（第二参是 value 或可见 label）"
                ))?;
                self.assert_no_dialog().await?;
                let (bn, owner) = self.lookup_ref_owner(r).await?;
                crate::semantic::select_option_in(&self.session, bn, value, owner.as_deref()).await
            }
            // 防呆勾选动词（#39）：checkRef 后必为 true，幂等
            name @ ("checkRef" | "uncheckRef") => {
                let want = name == "checkRef";
                let r = argv.first().and_then(Value::as_str).ok_or_else(|| anyhow!(
                    "{name} 缺 ref；下一步：{name}(\"e5\")（checkbox/radio 的 ref，来自 snapshot()）"
                ))?;
                self.assert_no_dialog().await?;
                let (bn, owner) = self.lookup_ref_owner(r).await?;
                crate::semantic::set_checked_in(&self.session, bn, want, owner.as_deref()).await
            }
            // ---- 对话框（吸收 agent-browser 语义）----
            "dialogStatus" => match self.session.pending_dialog().await {
                None => Ok(json!({ "open": false })),
                Some(ev) => Ok(json!({
                    "open": true,
                    "type": ev.pointer("/params/type"),
                    "message": ev.pointer("/params/message"),
                    "defaultPrompt": ev.pointer("/params/defaultPrompt"),
                })),
            },
            "dialogAccept" | "dialogDismiss" => {
                let accept = name == "dialogAccept";
                let ev = self
                    .session
                    .pending_dialog()
                    .await
                    .ok_or_else(|| anyhow!(
                        "当前没有打开的对话框（alert/beforeunload 已被自动接受）；下一步：confirm/prompt 由页面触发，先 return await dialogStatus() 确认"
                    ))?;
                let sid = ev
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow!("对话框事件缺 sessionId"))?
                    .to_string();
                let mut params = json!({ "accept": accept });
                if accept && let Some(t) = argv.first().and_then(Value::as_str) {
                    params["promptText"] = json!(t);
                }
                self.session
                    .call_on("Page.handleJavaScriptDialog", params, &sid)
                    .await?;
                self.session.clear_pending_dialog().await;
                Ok(json!(true))
            }
            // ---- 网络拦截（Fetch 域，watcher 应答 requestPaused）----
            "routeBlock" => {
                let pattern = argv.first().and_then(Value::as_str).ok_or_else(|| anyhow!(
                    "routeBlock 缺模式；下一步：routeBlock(\"*://ads.example.com/*\")（glob，* 通配）"
                ))?;
                self.routes.lock().await.push(RouteRule {
                    pattern: pattern.to_string(),
                    action: RouteAction::Block,
                });
                self.sync_fetch_patterns().await?;
                Ok(json!({ "rules": self.routes.lock().await.len() }))
            }
            "routeMock" => {
                let pattern = argv.first().and_then(Value::as_str).ok_or_else(|| anyhow!(
                    "routeMock 缺模式；下一步：routeMock(\"http://mock.test/api*\", \"<body>\", {{status:200, contentType:\"application/json\"}})"
                ))?;
                let body = argv.get(1).and_then(Value::as_str).ok_or_else(|| anyhow!(
                    "routeMock 缺应答体；下一步：第二参给 body 字符串（第三参可省：{{status, contentType}}）"
                ))?;
                let opts = argv.get(2);
                let action = RouteAction::Mock {
                    body: body.to_string(),
                    status: opts
                        .and_then(|o| o.get("status"))
                        .and_then(Value::as_i64)
                        .unwrap_or(200),
                    content_type: opts
                        .and_then(|o| o.get("contentType"))
                        .and_then(Value::as_str)
                        .unwrap_or("text/html")
                        .to_string(),
                    headers: {
                        let hs = opts
                            .and_then(|o| o.get("headers"))
                            .and_then(Value::as_object)
                            .map(|h| {
                                h.iter()
                                    .filter_map(|(k, v)| {
                                        v.as_str().map(|v| (k.clone(), v.to_string()))
                                    })
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        // 头名/头值预检（#38 评审 F1）：非法头名会让
                        // fulfillRequest 被 Chrome 拒执行而 requestPaused 无人
                        // 应答，页面静默挂到 30 秒超时
                        for (k, v) in &hs {
                            let token_ok = !k.is_empty()
                                && k.bytes().all(|b| {
                                    b.is_ascii_alphanumeric()
                                        || matches!(
                                            b,
                                            b'!' | b'#'
                                                | b'$'
                                                | b'%'
                                                | b'&'
                                                | b'\''
                                                | b'*'
                                                | b'+'
                                                | b'-'
                                                | b'.'
                                                | b'^'
                                                | b'_'
                                                | b'`'
                                                | b'|'
                                                | b'~'
                                        )
                                });
                            if !token_ok {
                                bail!(
                                    "routeMock headers 头名 {k:?} 不是合法 HTTP token（RFC 7230 字符集）；下一步：改头名，要自定义头跨源可读一并给 Access-Control-Expose-Headers"
                                );
                            }
                            if v.bytes().any(|b| b < 0x20 || b == 0x7f) {
                                bail!(
                                    "routeMock headers 头值含控制字符（{k}）；下一步：剔除控制字符"
                                );
                            }
                        }
                        hs
                    },
                };
                self.routes.lock().await.push(RouteRule {
                    pattern: pattern.to_string(),
                    action,
                });
                self.sync_fetch_patterns().await?;
                Ok(json!({ "rules": self.routes.lock().await.len() }))
            }
            "routeClear" => {
                self.routes.lock().await.clear();
                self.sync_fetch_patterns().await?;
                Ok(json!(true))
            }
            // ---- 存档：PDF（无头专属）----
            "pdf" => {
                let path = argv.first().and_then(Value::as_str).map(str::to_string);
                crate::semantic::pdf(&self.session, path.as_deref()).await
            }
            // ---- 录制（Page.startScreencast 帧流落盘）----
            "recordStart" => {
                let opts = argv.first().cloned().unwrap_or(json!({}));
                let mut rec = self.record.lock().await;
                if rec.is_some() {
                    bail!("已在录制（至多一场）；下一步：先 await recordStop() 收这一场，再开新的");
                }
                let r = crate::record::start(self.session.clone(), &opts).await?;
                // #43 视觉增强：cursor 画跟随光标元素、showActions 点击处闪
                // 圈（screencast 帧里可见，回放可读性）
                if opts.get("cursor").and_then(Value::as_bool).unwrap_or(false) {
                    let _ = self
                        .session
                        .call(
                            "Runtime.evaluate",
                            json!({
                                "expression": "(() => { let c = document.getElementById('browse-rec-cursor'); if (!c) { c = document.createElement('div'); c.id = 'browse-rec-cursor'; c.style.cssText = 'position:absolute;width:14px;height:14px;border-radius:50%;background:rgba(255,140,0,.85);border:2px solid #fff;z-index:2147483647;pointer-events:none;transition:left .08s,top .08s'; document.documentElement.appendChild(c); } return true })()",
                                "returnByValue": true
                            }),
                        )
                        .await;
                }
                if opts
                    .get("showActions")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    let _ = self
                        .session
                        .call(
                            "Runtime.evaluate",
                            json!({
                                "expression": "(() => { if (window.__browseShowActions) return true; const h = (e) => { const f = document.createElement('div'); f.style.cssText = 'position:absolute;left:' + (e.pageX - 22) + 'px;top:' + (e.pageY - 22) + 'px;width:44px;height:44px;border-radius:50%;border:3px solid #ff8c00;z-index:2147483647;pointer-events:none;transition:transform .5s,opacity .5s'; document.documentElement.appendChild(f); requestAnimationFrame(() => { f.style.transform = 'scale(2.2)'; f.style.opacity = '0'; }); setTimeout(() => f.remove(), 600); }; window.__browseShowActions = h; addEventListener('click', h, true); return true })()",
                                "returnByValue": true
                            }),
                        )
                        .await;
                }
                let brief = r.brief();
                *rec = Some(r);
                Ok(brief)
            }
            "recordChapter" => {
                // #43 章节标记：录制中插章节，落 record 目录 chapters.jsonl
                let title = str_arg(argv, 0, "recordChapter 的标题")?;
                let rec = self.record.lock().await;
                let r = rec.as_ref().ok_or_else(|| {
                    anyhow!("当前没有录制；下一步：先 await recordStart() 再插章节")
                })?;
                let frames = crate::record::chapter(r, title)?;
                Ok(json!({ "chapter": title, "atFrames": frames }))
            }
            "recordStop" => {
                let rec = self
                    .record
                    .lock()
                    .await
                    .take()
                    .ok_or_else(|| anyhow!(
                        "当前没有录制；下一步：先 await recordStart({{everyNthFrame:2}}) 开一场（可选抽帧/限宽）"
                    ))?;
                let r = crate::record::stop(&self.session, rec).await?;
                // #43 清场：光标元素移除（showActions 的监听随 stopScreencast
                // 后的整页卸载/下一次 recordStart 才彻底消，闪烁圈自移除）
                let _ = self
                    .session
                    .call(
                        "Runtime.evaluate",
                        json!({
                            "expression": "(() => { document.getElementById('browse-rec-cursor')?.remove(); if (window.__browseShowActions) { removeEventListener('click', window.__browseShowActions, true); delete window.__browseShowActions; } return true })()",
                            "returnByValue": true
                        }),
                    )
                    .await;
                Ok(r)
            }
            other => bail!(
                "未知函数 {other}；下一步：可用全局 {GLOBALS_CTA}；CDP 走 session.<Domain>.<method>(params)"
            ),
        }
    }

    /// 查短 ref 对应的 backendNodeId（只认最近一次 snapshot 的表），
    /// 并做主动代际校验（daemon 侧，#30 消注入痕）。
    ///
    /// 快照时记下 session 与文档代（[`cdp::Session::doc_generation`]，
    /// 主框架导航递增），引用前核对两者：换过 tab 或主框架导航过即整表
    /// 作废，给重取 CTA。SPA 同文档跳转（pushState）代不动，ref 继续有效。
    /// 活动缺席（None，未走 use 的裸态）按过期处理，给重取 CTA（比旧
    /// 标记口径严：旧口径取不到标记不拦、退被动兜底）。Page 域没开的
    /// 高级手写 attach 路径导航事件不流动、代不动，校验退化恒过，靠
    /// 被动兜底（resolveNode/零尺寸）。
    async fn lookup_ref(&self, r: &str) -> Result<i64> {
        let table = self.refs.lock().await.clone();
        let Some(t) = table else {
            bail!(
                "还没有元素引用（引用表来自 snapshot()）；下一步：先 await snapshot()，用返回里 nodes[].ref"
            );
        };
        let Some(bn) = t.map.get(r).map(|e| e.backend_node_id) else {
            bail!(
                "未知 ref {r}（引用表只保留最近一次 snapshot()）；下一步：先 await snapshot()，用返回里 nodes[].ref"
            );
        };
        // 活动还是快照那个 tab 且文档代没动过才放行：换 tab 后 backendNodeId
        // 跨 target 无意义（代数同值也不可信），导航后主文档已换
        let cur = self.session.get_active_session().await;
        let fresh = match cur.as_deref() {
            Some(sid) if sid == t.session_id => {
                self.session.doc_generation(sid).await == t.generation
            }
            _ => false,
        };
        if !fresh {
            bail!(
                "ref 已过期（换过 tab 或页面文档已换代，旧 ref 全体作废）；下一步：重新 await snapshot() 取新 ref"
            );
        }
        Ok(bn)
    }

    /// 同 [`JsHost::lookup_ref`]，但把「归属子 session」一并带出（#60）：
    /// OOPIF 节点要在自己的子 session 上量布局/填值，调用方据此选通道。
    async fn lookup_ref_owner(&self, r: &str) -> Result<(i64, Option<String>)> {
        let table = self.refs.lock().await.clone();
        let Some(t) = table else {
            bail!(
                "还没有元素引用（引用表来自 snapshot()）；下一步：先 await snapshot()，用返回里 nodes[].ref"
            );
        };
        let Some(e) = t.map.get(r).cloned() else {
            bail!(
                "未知 ref {r}（引用表只保留最近一次 snapshot()）；下一步：先 await snapshot()，用返回里 nodes[].ref"
            );
        };
        let cur = self.session.get_active_session().await;
        let fresh = match cur.as_deref() {
            Some(sid) if sid == t.session_id => {
                self.session.doc_generation(sid).await == t.generation
            }
            _ => false,
        };
        if !fresh {
            bail!(
                "ref 已过期（换过 tab 或页面文档已换代，旧 ref 全体作废）；下一步：重新 await snapshot() 取新 ref"
            );
        }
        Ok((e.backend_node_id, e.session.clone()))
    }

    /// 活动页的 url/title（#30 消注入痕）：主源 `Target.getTargetInfo`
    /// （browser 级查询，attach 面全经 use_target、targetId 在册），
    /// 不在册或失败回落 `Page.getNavigationHistory` 当前条目；都失败给
    /// 空串——url/title 是快照的展示字段，不为它炸整次 snapshot。
    async fn page_meta(&self) -> (String, String) {
        if let Some(tid) = self.session.active_target().await
            && let Ok(r) = self
                .session
                .call("Target.getTargetInfo", json!({ "targetId": tid }))
                .await
            && let Some(info) = r.get("targetInfo")
        {
            let url = info.get("url").and_then(Value::as_str).unwrap_or_default();
            let title = info
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !url.is_empty() {
                return (url.to_string(), title.to_string());
            }
        }
        if let Ok(r) = self
            .session
            .call("Page.getNavigationHistory", json!({}))
            .await
            && let Some(entries) = r.get("entries").and_then(Value::as_array)
            && let Some(ci) = r.get("currentIndex").and_then(Value::as_u64)
            && let Some(entry) = entries.get(ci as usize)
        {
            return (
                entry
                    .get("url")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                entry
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            );
        }
        (String::new(), String::new())
    }

    async fn call_session(&self, method: &str, argv: &[Value]) -> Result<Value> {
        match method {
            "connect" => {
                if self.session.is_connected() {
                    return Ok(json!({"ok": true, "already": true}));
                }
                let opts = connect_opts(argv.first());
                self.session.connect_opts(opts).await?;
                Ok(json!({"ok": true}))
            }
            "use" => {
                let id = argv
                    .first()
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow!(
                        "session.use 需要 targetId 字符串；下一步：session.use(tabs[0].targetId)，先 const tabs = await listPageTargets()"
                    ))?;
                let sid = self.session.use_target(id).await?;
                ensure_page_enabled(&self.session, &sid).await;
                Ok(json!(sid))
            }
            "waitFor" | "wait_for" => {
                let ev = argv
                    .first()
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow!(
                        "waitFor 缺 method 字符串；下一步：await session.waitFor(\"Page.frameNavigated\", undefined, 15)（秒）。注意 loadEventFired 有竞速窗：事件在注册前已发则假超时（#19），等加载用 goto() 或 waitLoad()，等导航事件用 frameNavigated"
                    ))?;
                // 秒口径（#51）：缺省 15 秒；位置 2 优先、位置 1 容忍旧两参
                // 形 waitFor(method, ms)（评审 G3）；旧毫秒习惯值由混用守卫
                // 换算并告警（事件回执是 CDP 原形，告警走 daemon 留痕）
                let raw = argv.get(2).filter(|v| !v.is_null());
                let raw = match raw {
                    Some(v) => Some(v),
                    // 两参旧形：位置 1 是数值即 timeout，是 matcher 串则忽略
                    None => argv.get(1).filter(|v| v.as_u64().is_some()),
                };
                let (ms, warn) = match raw {
                    None => secs_to_ms(15),
                    Some(v) => {
                        let n = v.as_u64().ok_or_else(|| anyhow!(
                            "waitFor 的 timeout 应是整秒数（当前：{}）；下一步：session.waitFor(\"Page.frameNavigated\", undefined, 15)",
                            preview(v)
                        ))?;
                        secs_to_ms(n)
                    }
                };
                if let Some(w) = warn {
                    eprintln!("[browse] waitFor {w}");
                }
                self.session.wait_for(ev, ms).await
            }
            "call" => {
                let method = argv
                    .first()
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow!(
                        "session.call 缺 method 字符串；下一步：await session.call(\"Page.navigate\", {{url:\"https://example.com\"}})"
                    ))?;
                let params = argv.get(1).cloned().unwrap_or(json!({}));
                self.session.call(method, params).await
            }
            "isConnected" => Ok(json!(self.session.is_connected())),
            // 页内谓词等待（对齐 pi 的 page.waitFor / 官方 waitFor 的谓词面在
            // 无函数方言下的代偿）：轮询 Runtime.evaluate 直到表达式真值
            "waitJs" | "wait_js" => {
                let expr = argv
                    .first()
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow!(
                        "waitJs 缺 expression 字符串；下一步：await session.waitJs(\"document.querySelector('#x') !== null\", 5)（页内表达式真值即通过，秒）"
                    ))?;
                // 秒口径（#51）：缺省 10 秒；返回值是页内原值非对象，混用
                // 告警走 daemon 留痕
                let (timeout_ms, warn) = timeout_ms_of(argv, 1, 10, "waitJs")?;
                if let Some(w) = warn {
                    eprintln!("[browse] waitJs {w}");
                }
                let deadline =
                    tokio::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
                let mut last_err = String::new();
                loop {
                    let r = self
                        .session
                        .call(
                            "Runtime.evaluate",
                            json!({ "expression": expr, "returnByValue": true }),
                        )
                        .await;
                    match r {
                        Ok(resp) => {
                            let truthy = resp.pointer("/result/value").is_some_and(|v| {
                                !v.is_null()
                                    && *v != json!(false)
                                    && *v != json!(0)
                                    && *v != json!("")
                            });
                            if truthy {
                                return Ok(resp
                                    .pointer("/result/value")
                                    .cloned()
                                    .unwrap_or(Value::Null));
                            }
                        }
                        Err(e) => last_err = format!("{e:#}"),
                    }
                    if tokio::time::Instant::now() >= deadline {
                        return Err(anyhow!(
                            "waitJs 超时（{} 秒）：{expr}{}",
                            timeout_ms / 1000,
                            if last_err.is_empty() {
                                String::new()
                            } else {
                                format!("；最后一次求值错误：{last_err}")
                            }
                        ));
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                }
            }
            "getActiveSession" => Ok(json!(self.session.get_active_session().await)),
            "close" => {
                self.session.close().await;
                Ok(json!(true))
            }
            "setActiveSession" => {
                let sid = argv.first().and_then(Value::as_str).map(str::to_string);
                self.session.set_active_session(sid).await;
                Ok(json!(true))
            }
            "peekEvents" => {
                let m = argv
                    .first()
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow!(
                        "peekEvents 缺 method 字符串；下一步：await session.peekEvents(\"Page.frameStartedLoading\", 3)"
                    ))?;
                let n = argv.get(1).and_then(Value::as_u64).unwrap_or(1) as usize;
                Ok(Value::Array(self.session.peek_events(m, n).await))
            }
            "peekEventsSince" => {
                let m = argv.first().and_then(Value::as_str).ok_or_else(|| {
                    anyhow!(
                        "peekEventsSince 缺 method 字符串；下一步：await session.peekEventsSince(\"Network.requestWillBeSent\", 0, 5)（首拍 sinceSeq 用 0）"
                    )
                })?;
                let since = argv.get(1).and_then(Value::as_u64).ok_or_else(|| {
                    anyhow!(
                        "peekEventsSince 第二参要是 seq 数字；下一步：用上一拍最后一条事件的 seq（首拍用 0）"
                    )
                })?;
                let n = argv.get(2).and_then(Value::as_u64).unwrap_or(1) as usize;
                Ok(Value::Array(
                    self.session.peek_events_since(m, since, n).await,
                ))
            }
            "findEvents" => {
                let m = argv.first().and_then(Value::as_str).ok_or_else(|| {
                    anyhow!(
                        "findEvents 缺 method 字符串；下一步：await session.findEvents(\"Network.responseReceived\", \"params.response.status\", 200, 1)"
                    )
                })?;
                let path = argv.get(1).and_then(Value::as_str).ok_or_else(|| {
                    anyhow!(
                        "findEvents 第二参要是点分路径字符串；下一步：例如 \"params.requestId\"、\"params.response.status\""
                    )
                })?;
                let val = argv.get(2).cloned().unwrap_or(Value::Null);
                let n = argv.get(3).and_then(Value::as_u64).unwrap_or(1) as usize;
                Ok(Value::Array(
                    self.session.find_events(m, path, &val, n).await,
                ))
            }
            other => bail!(
                "未知 session.{other}；下一步：宿主面 connect/close/use/setActiveSession/waitFor/call/peekEvents/peekEventsSince/findEvents/isConnected/getActiveSession；便捷函数（waitLoad/waitIdle/waitForResponse/responseBody/routeMock 等）是全局，直接调不带 session. 前缀；CDP 域写 session.<Domain>.<method>(params)"
            ),
        }
    }
}

/// 对话框 watcher（游离常驻任务，吸收 agent-browser 语义）：自动接受
/// `alert`/`beforeunload`，永不阻塞 agent。
///
/// 给新活动 session 补 `Page.enable`（对话框事件需要域开启才流动）；
/// `BROWSE_NO_AUTO_DIALOG=1` 关掉自动接受；`confirm`/`prompt` 留给显式
/// `dialogAccept/dialogDismiss`。状态由 cdp `route()` 截获维护
/// （[`cdp::Session::pending_dialog`]）。
///
/// webdriver 覆写不在此做（用户裁定 2026-09-18）：等 clean-chrome 源码级
/// 恒 false（其 155 窗资产，四态免疫）；155 前需 Google 登录的场附着
/// 正式版 Chrome。
fn spawn_dialog_watcher(
    session: Arc<Session>,
    init_script: Arc<std::sync::Mutex<Option<String>>>,
    init_script_ids: Arc<Mutex<HashMap<String, Vec<String>>>>,
) {
    tokio::spawn(async move {
        let auto =
            !std::env::var_os("BROWSE_NO_AUTO_DIALOG").is_some_and(|v| v == "1" || v == "true");
        let mut enabled: HashSet<String> = HashSet::new();
        // 连接纪元观察（全量评审 G1）：重连（引擎换代）后旧 sid 的
        // identifier 全是陈尸，随换代清表，与 cdp 侧重连清账同呼吸；
        // 新 session 的补注由本 watcher 下一拍照常做
        let mut seen_epoch = session.connection_epoch();
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            let epoch = session.connection_epoch();
            if epoch != seen_epoch {
                seen_epoch = epoch;
                init_script_ids.lock().await.clear();
                enabled.clear();
            }
            // #60：OOPIF 目标出现（targetCreated 记账）即显式附着成子
            // session；无待附着时不打 CDP（廉价触发）
            if session.has_pending_children().await {
                let _ = session.sync_child_sessions().await;
            }
            // 活动路由换 session 后补开域（幂等）
            if let Some(sid) = session.get_active_session().await
                && !enabled.contains(&sid)
                && session
                    .call_on("Page.enable", json!({}), &sid)
                    .await
                    .is_ok()
            {
                // 每新 session 补注 init 脚本（#25.2）：已设则新文档生效，
                // identifier 记账（替换/清除要 remove，评审 F1）
                if let Some(code) = init_script.lock().ok().and_then(|g| g.clone())
                    && let Ok(r) = session
                        .call_on(
                            "Page.addScriptToEvaluateOnNewDocument",
                            json!({ "source": code }),
                            &sid,
                        )
                        .await
                    && let Some(ident) = r
                        .get("identifier")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                {
                    init_script_ids
                        .lock()
                        .await
                        .entry(sid.clone())
                        .or_default()
                        .push(ident);
                }
                enabled.insert(sid);
            }
            if !auto {
                continue;
            }
            if let Some(ev) = session.pending_dialog().await {
                let ty = ev
                    .pointer("/params/type")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if matches!(ty, "alert" | "beforeunload")
                    && let Some(sid) = ev.get("sessionId").and_then(Value::as_str)
                {
                    let _ = session
                        .call_on(
                            "Page.handleJavaScriptDialog",
                            json!({ "accept": true }),
                            sid,
                        )
                        .await;
                }
            }
        }
    });
}

/// 网络拦截 watcher（游离常驻任务）：由它按 [`RouteRule`] 应答必须应答的
/// `Fetch.requestPaused` 事件（不应答页面就挂着）。
///
/// 命中的 failRequest/fulfillRequest，未命中的 continueRequest 放行；
/// 只在我们自己 `Fetch.enable` 时才接管（手动开 Fetch 域的 requestPaused
/// 不动，留给 agent 自己 peekEvents 处理）。
fn spawn_route_watcher(
    session: Arc<Session>,
    routes: Arc<Mutex<Vec<RouteRule>>>,
    fetch_by_us: Arc<std::sync::atomic::AtomicBool>,
) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            if !fetch_by_us.load(Ordering::Relaxed) {
                continue;
            }
            for ev in session.drain_events("Fetch.requestPaused").await {
                let Some(sid) = ev.get("sessionId").and_then(Value::as_str) else {
                    continue;
                };
                let Some(rid) = ev
                    .pointer("/params/requestId")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                else {
                    continue;
                };
                let url = ev
                    .pointer("/params/request/url")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let action = routes
                    .lock()
                    .await
                    .iter()
                    .find(|r| glob_match(&r.pattern, url))
                    .map(|r| r.action.clone());
                match action {
                    Some(RouteAction::Block) => {
                        let _ = session
                            .call_on(
                                "Fetch.failRequest",
                                json!({ "requestId": rid, "errorReason": "BlockedByClient" }),
                                sid,
                            )
                            .await;
                    }
                    Some(RouteAction::Mock {
                        body,
                        status,
                        content_type,
                        headers,
                    }) => {
                        // mock 是测试原语：默认带 ACAO，跨源页（data: 测试页）也读得到
                        let mut rh = vec![
                            json!({ "name": "Content-Type", "value": content_type }),
                            json!({ "name": "Access-Control-Allow-Origin", "value": "*" }),
                        ];
                        for (k, v) in headers {
                            rh.push(json!({ "name": k, "value": v }));
                        }
                        if let Err(e) = session
                            .call_on(
                                "Fetch.fulfillRequest",
                                json!({
                                    "requestId": rid,
                                    "responseCode": status,
                                    "body": base64_encode(body.as_bytes()),
                                    "responseHeaders": rh,
                                }),
                                sid,
                            )
                            .await
                        {
                            // 兑付失败升级留痕（#38 评审 F1）：静默吞会让
                            // requestPaused 无人应答、页面挂到超时无线索
                            eprintln!(
                                "[browse] routeMock 兑付失败（该请求将挂起至超时，检查规则的头与值）：{e:#}"
                            );
                        }
                    }
                    None => {
                        let _ = session
                            .call_on("Fetch.continueRequest", json!({ "requestId": rid }), sid)
                            .await;
                    }
                }
            }
        }
    });
}

/// 极简 glob：`*` 任意串（可多个），无 `*` 即等值；首段锚头、末段锚尾。
fn glob_match(pat: &str, text: &str) -> bool {
    let segs: Vec<&str> = pat.split('*').collect();
    if segs.len() == 1 {
        return pat == text;
    }
    let head = segs[0];
    let tail = segs[segs.len() - 1];
    let Some(rest) = text.strip_prefix(head) else {
        return false;
    };
    let Some(body) = rest.strip_suffix(tail) else {
        return false;
    };
    let mut cur = body;
    for seg in &segs[1..segs.len() - 1] {
        if seg.is_empty() {
            continue;
        }
        match cur.find(seg) {
            Some(i) => cur = &cur[i + seg.len()..],
            None => return false,
        }
    }
    true
}

/// 标准字母表加 padding 的 base64 编码，与 [`base64_decode`] 对偶。
pub(crate) fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(TABLE[(n >> 18 & 63) as usize] as char);
        out.push(TABLE[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6 & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn tab_json(t: PageTarget) -> Value {
    json!({
        "targetId": t.target_id,
        "title": t.title,
        "url": t.url,
        "type": t.type_,
        "own": t.own,
    })
}

fn connect_opts(v: Option<&Value>) -> ConnectOptions {
    let Some(v) = v else {
        return ConnectOptions {
            port: Some(9222),
            ..Default::default()
        };
    };
    ConnectOptions {
        ws_url: v
            .get("wsUrl")
            .or_else(|| v.get("url"))
            .and_then(Value::as_str)
            .map(|s| s.to_string()),
        port: v
            .get("port")
            .and_then(Value::as_u64)
            .map(|n| n as u16)
            .or_else(|| v.as_u64().map(|n| n as u16)),
        profile_dir: v
            .get("profileDir")
            .and_then(Value::as_str)
            .map(|s| s.to_string()),
        timeout_ms: v.get("timeoutMs").and_then(Value::as_u64),
    }
}

// ---- 值方法面与 JSON 命名空间（#21）：方言结果在宿主侧的小加工 ----
// 纯函数、无控制流；页面内逻辑仍走 Runtime.evaluate（分工见 --llms 手册）。

/// 取 Runtime.evaluate 结果里的异常描述（exceptionDetails 的 description
/// 优先、text 兜底），无异常返回 None。
fn exception_text(r: &Value) -> Option<&str> {
    r.get("exceptionDetails")?
        .pointer("/exception/description")
        .or_else(|| r.get("exceptionDetails")?.get("text"))
        .and_then(Value::as_str)
}

/// 把 `Network.getResponseBody` 的返回解码成 `{body, base64Encoded}`：
/// base64 响应自动解码为 UTF-8 文本（#20）。
///
/// 体就绪等待窗（毫秒）：测试态收短防拖慢单测。
#[cfg(test)]
const BODY_WAIT_MS: u64 = 800;
#[cfg(not(test))]
const BODY_WAIT_MS: u64 = 5_000;

fn decode_response_body(b: &Value) -> Value {
    let body = b.get("body").and_then(Value::as_str).unwrap_or("");
    let b64 = b
        .get("base64Encoded")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let text = if b64 {
        base64_decode(body)
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_else(|_| body.to_string())
    } else {
        body.to_string()
    };
    json!({ "body": text, "base64Encoded": b64 })
}

/// 换靶或开新靶后同步补开 Page 域（#19）：消灭「use 之后立即 waitFor」
/// 的开域时序竞态——事件只在 Page 域已开时才投递，300ms 轮询 watcher
/// 是兜底不是主路径。幂等；失败静默（非 Page 型 target），不阻断换靶。
///
/// 防漏口径（#19 评审留痕）：新增 `use_target` 调用点必须同步补挂本
/// 函数，当前五处——js_host 的 `use` 臂、semantic 的 `new_tab` 与
/// `switch_tab`、engine 的 `attach_first_page` 与 `new_tab`；高级路径
/// （手写 `Target.attachToTarget` 加 `setActiveSession`）豁免，靠
/// watcher 兜底。
pub(crate) async fn ensure_page_enabled(session: &Session, sid: &str) {
    let _ = session.call_on("Page.enable", json!({}), sid).await;
    // #37 可观测三件自动开域：console/jsErrors 要 Runtime、requests 要
    // Network。幂等；晚开域之前的旧事件收不到（自开域后起算）
    let _ = session.call_on("Runtime.enable", json!({}), sid).await;
    let _ = session.call_on("Network.enable", json!({}), sid).await;
    // #60 OOPIF：setAutoAttach 只当补充（本机 Chrome 实测不产 iframe 型
    // 事件），真正的发现在 setDiscoverTargets + sync_child_sessions：
    // 跨域 iframe 目标出现即记账（route 里 Target.targetCreated），这里与
    // pierce 时再显式 attach 一遍
    let _ = session.enable_auto_attach().await;
    let _ = session
        .call("Target.setDiscoverTargets", json!({ "discover": true }))
        .await;
    let _ = session.sync_child_sessions().await;
    // #38 下载捕获：落盘到状态目录 downloads 子目录并开事件（browser 级
    // 幂等）
    let _ = session
        .call(
            "Browser.setDownloadBehavior",
            json!({
                "behavior": "allow",
                "downloadPath": crate::paths::state_dir().join("downloads"),
                "eventsEnabled": true,
            }),
        )
        .await;
}

/// 字符串方法面清单（#21）：CTA 文案由此派生，surface 目录描述由测试绑定；
/// 增删方法改这里（两边一起红才是同步）。
pub(crate) const STRING_METHODS: &[&str] = &[
    "slice(start,end?)",
    "split(sep)",
    "includes(sub)",
    "startsWith(sub)",
    "endsWith(sub)",
    "trim()",
    "toUpperCase()",
    "toLowerCase()",
];

/// 数组方法面清单（#21）：同字符串清单的同源纪律。
pub(crate) const ARRAY_METHODS: &[&str] = &[
    "slice(start,end?)",
    "join(sep?)",
    "includes(v)",
    "concat(数组...)",
];

/// 值等值（数组 includes 用，#21）：JSON 等值之外数值跨形态宽等，
/// 1 与 1.0 同值（对齐 JS）；整数段 i64/u64 精确比，浮点兜底。
fn json_value_eq(a: &Value, b: &Value) -> bool {
    if a == b {
        return true;
    }
    let (Value::Number(x), Value::Number(y)) = (a, b) else {
        return false;
    };
    if let (Some(p), Some(q)) = (x.as_i64(), y.as_i64()) {
        return p == q;
    }
    if let (Some(p), Some(q)) = (x.as_u64(), y.as_u64()) {
        return p == q;
    }
    x.as_f64().zip(y.as_f64()).is_some_and(|(p, q)| p == q)
}

/// JS 语义的索引规约：负数从尾部数，钳到 [0, len]。
fn js_index(i: i64, len: usize) -> usize {
    let len = len as i64;
    (if i < 0 { len + i } else { i }).clamp(0, len) as usize
}

/// 字符串按 UTF-16 单元切片（与 `.length` 同口径，JS slice 语义；
/// start 不小于 end 得空串，切在代理对中间由 from_utf16_lossy 收尾）。
fn js_slice_units(s: &str, start: i64, end: Option<i64>) -> String {
    let units: Vec<u16> = s.encode_utf16().collect();
    let a = js_index(start, units.len());
    let b = js_index(end.unwrap_or(units.len() as i64), units.len());
    if a >= b {
        return String::new();
    }
    String::from_utf16_lossy(&units[a..b])
}

/// 数组切片（同上索引语义，元素克隆）。
fn js_slice_items<T: Clone>(items: &[T], start: i64, end: Option<i64>) -> Vec<T> {
    let a = js_index(start, items.len());
    let b = js_index(end.unwrap_or(items.len() as i64), items.len());
    if a >= b {
        Vec::new()
    } else {
        items[a..b].to_vec()
    }
}

/// 取第 i 个字符串参数，带形态化 CTA。
fn str_arg<'a>(argv: &'a [Value], i: usize, who: &str) -> Result<&'a str> {
    argv.get(i).and_then(Value::as_str).ok_or_else(|| {
        anyhow!(
            "{who} 应是字符串；下一步：检查实参形态（当前：{}）",
            preview(argv.get(i).unwrap_or(&Value::Null))
        )
    })
}

/// 事件是否来自当前活动 tab（#37 评审 F1，口径同 wait_for_response 的
/// #33 F2）：他 tab 的 console/异常/请求信号不泄漏；无活动 session 时只
/// 认 browser 级（无 sessionId）。
fn from_active_session(active: &Option<String>, e: &Value) -> bool {
    match active {
        Some(a) => e.get("sessionId").and_then(Value::as_str) == Some(a.as_str()),
        None => e.get("sessionId").is_none(),
    }
}

/// AX 节点 id/childIds 归一解析（#36）：真 chrome 回执是数字串（"2"），
/// 假对端与文档示例是数字，两形都吃。
fn ax_id(x: &Value) -> Option<i64> {
    x.as_i64()
        .or_else(|| x.as_str().and_then(|s| s.parse().ok()))
}

/// wait 类 timeout 秒值换毫秒（#51 统一口径）：不小于 1000 视为毫秒误写，
/// 按毫秒换算（15000 即 15 秒）并出告警，换算后封顶 600 秒；三位数内按秒
/// 原样放大（上界 999 秒）。判据从大于 3600 收到 1000（#57 F1）：1000 至
/// 3600 的秒直解静默窗已实弹炸在本仓测试（300 意图变 300 秒），真实等待
/// 三位数秒封顶足够。返回 (毫秒, 告警)，告警由调用方附进结果对象（无对象
/// 面走 daemon 留痕）。
fn secs_to_ms(v: u64) -> (u64, Option<String>) {
    const MISUSE: u64 = 1000;
    const CAP: u64 = 600;
    if v >= MISUSE {
        let s = (v / 1000).clamp(1, CAP);
        (
            s * 1000,
            Some(format!(
                "timeout {v} 不小于 1000，按毫秒误写换算为 {s} 秒（封顶 600）；秒口径直写三位数内如 waitLoad(15)"
            )),
        )
    } else {
        (v * 1000, None)
    }
}

/// 取第 i 个 wait 类 timeout 实参（缺省 default_s 秒）并过 [`secs_to_ms`]
/// 混用守卫；实参在位但不是整数秒形态即报错（评审 G4：不静默落缺省）。
fn timeout_ms_of(
    argv: &[Value],
    i: usize,
    default_s: u64,
    who: &str,
) -> Result<(u64, Option<String>)> {
    match argv.get(i) {
        None | Some(Value::Null) => Ok(secs_to_ms(default_s)),
        Some(v) => v.as_u64().map(secs_to_ms).ok_or_else(|| anyhow!(
            "{who} 的 timeout 应是整秒数（当前：{}）；下一步：秒口径直写如 waitLoad(15)，旧毫秒习惯值大于 3600 自动换算",
            preview(v)
        )),
    }
}

/// 把混用告警附进结果对象（只对对象面结果有意义；附不上的调用方走
/// eprintln 留痕）。
fn attach_warning(mut r: Value, warn: Option<String>) -> Value {
    if let (Some(w), Some(obj)) = (warn, r.as_object_mut()) {
        obj.insert("timeoutWarning".to_string(), json!(w));
    }
    r
}

/// fill 类第三参的 submit 判定：布尔 true 或对象 {submit: true}。
fn submit_arg(v: Option<&Value>) -> bool {
    match v {
        Some(Value::Bool(true)) => true,
        Some(o @ Value::Object(_)) => o.get("submit").and_then(Value::as_bool) == Some(true),
        _ => false,
    }
}

/// 取第 i 个整数参数（布尔与浮点不接受），带 CTA。
fn int_arg(argv: &[Value], i: usize, who: &str) -> Result<i64> {
    argv.get(i).and_then(Value::as_i64).ok_or_else(|| {
        anyhow!(
            "{who} 应是整数；下一步：检查实参形态（当前：{}）",
            preview(argv.get(i).unwrap_or(&Value::Null))
        )
    })
}

/// 取可选的第 i 个整数参数（缺省或 null 得 None）。
fn opt_int_arg(argv: &[Value], i: usize, who: &str) -> Result<Option<i64>> {
    match argv.get(i) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => int_arg(argv, i, who).map(Some),
    }
}

/// 值方法面入口：字符串与数组的成员调用；非该方法面的成员返回 None，
/// 由调用方落类型感知 CTA。
fn value_method(o: &Value, prop: &str, argv: &[Value]) -> Option<Result<Value>> {
    match o {
        Value::String(s) => string_method(s, prop, argv),
        Value::Array(a) => array_method(a, prop, argv),
        _ => None,
    }
}

/// 字符串方法面：slice 按UTF-16 单元（同 `.length` 口径），大小写转换按
/// 字符非 locale 敏感。
fn string_method(s: &str, prop: &str, argv: &[Value]) -> Option<Result<Value>> {
    Some(match prop {
        "slice" => (|| {
            let start = int_arg(argv, 0, "slice 的 start")?;
            let end = opt_int_arg(argv, 1, "slice 的 end")?;
            Ok(json!(js_slice_units(s, start, end)))
        })(),
        "split" => (|| {
            let sep = str_arg(argv, 0, "split 的分隔符")?;
            if sep.is_empty() {
                bail!(
                    "split 的分隔符不能是空串；下一步：逐字符拆解走页面侧 Runtime.evaluate 或 shell 处理"
                );
            }
            Ok(json!(s.split(sep).map(|p| json!(p)).collect::<Vec<_>>()))
        })(),
        "includes" => (|| Ok(json!(s.contains(str_arg(argv, 0, "includes 的子串")?))))(),
        "startsWith" => (|| Ok(json!(s.starts_with(str_arg(argv, 0, "startsWith 的前缀")?))))(),
        "endsWith" => (|| Ok(json!(s.ends_with(str_arg(argv, 0, "endsWith 的后缀")?))))(),
        "trim" => Ok(json!(s.trim())),
        "toUpperCase" => Ok(json!(s.to_uppercase())),
        "toLowerCase" => Ok(json!(s.to_lowercase())),
        _ => return None,
    })
}

/// 数组方法面：join 对容器元素打紧凑 JSON（与 JS 的 [object Object] 不同，
/// 取对 agent 更有用的形态）。
fn array_method(a: &[Value], prop: &str, argv: &[Value]) -> Option<Result<Value>> {
    Some(match prop {
        "slice" => (|| {
            let start = int_arg(argv, 0, "slice 的 start")?;
            let end = opt_int_arg(argv, 1, "slice 的 end")?;
            Ok(Value::Array(js_slice_items(a, start, end)))
        })(),
        "join" => {
            let sep = argv.first().and_then(Value::as_str).unwrap_or(",");
            let parts: Vec<String> = a
                .iter()
                .map(|v| match v {
                    Value::Null => String::new(),
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect();
            Ok(json!(parts.join(sep)))
        }
        "includes" => (|| {
            let needle = argv.first().cloned().ok_or_else(|| {
                anyhow!("includes 应带一个待查值；下一步：includes(v) 按值等值比对")
            })?;
            Ok(json!(a.iter().any(|x| json_value_eq(x, &needle))))
        })(),
        "concat" => (|| {
            let mut out = a.to_vec();
            for (i, v) in argv.iter().enumerate() {
                match v {
                    Value::Array(xs) => out.extend(xs.iter().cloned()),
                    _ => bail!(
                        "concat 的第 {} 个实参应是数组；下一步：concat(数组...) 逐个拼接（当前：{}）",
                        i + 1,
                        preview(v)
                    ),
                }
            }
            Ok(Value::Array(out))
        })(),
        _ => return None,
    })
}

/// JSON 命名空间（#21）：`JSON.parse` 与 `JSON.stringify` 做宿主侧的
/// 序列化往返（结果小加工、回传 shell）。
fn call_json_ns(prop: &str, argv: &[Value]) -> Result<Value> {
    match prop {
        "parse" => {
            let s = str_arg(argv, 0, "JSON.parse 的入参")?;
            serde_json::from_str(s).map_err(|e| anyhow!(
                "不是合法 JSON（{e}）；下一步：先 await print(x) 看原值形态再解析；CDP getResponseBody 的 body 已是解码后字符串"
            ))
        }
        "stringify" => {
            let v = argv.first().cloned().unwrap_or(Value::Null);
            let indent = opt_int_arg(argv, 1, "JSON.stringify 的 indent")?.unwrap_or(0);
            if indent > 0 {
                Ok(json!(serde_json::to_string_pretty(&v)?))
            } else {
                Ok(json!(v.to_string()))
            }
        }
        other => {
            bail!("JSON.{other} 不存在；下一步：JSON 只有 parse(字符串) 与 stringify(值, indent?)")
        }
    }
}

/// 预览截断帽（字符数）：超长截断并标注总长。
const PREVIEW_MAX_CHARS: usize = 24;

/// 按预览帽截断，超长时尾注总字符数。
fn trunc_preview(s: &str) -> String {
    let n = s.chars().count();
    if n > PREVIEW_MAX_CHARS {
        format!(
            "{}…(共 {n} 字符)",
            s.chars().take(PREVIEW_MAX_CHARS).collect::<String>()
        )
    } else {
        s.to_string()
    }
}

/// 容器预览只序列化的头部元素数（#33 F5）：报错路径不做全量序列化尖峰。
const PREVIEW_HEAD_ITEMS: usize = 8;

/// 全局函数 CTA 清单（#33 G6 单一真相）：「未知函数」提示由此派生，
/// `global_cta_covers_catalog` 测试把它与 surface 目录的 Global 条目绑死；
/// 增删全局必须同步这里（value-methods 是方法面族条目，不在此列）。
const GLOBALS_CTA: &str = "listPageTargets()/resolveWsUrl()/detectBrowsers()/cdpMethods(domain?)/hostFunctions()/snapshot(opts?)/findRefs(q,opts?)/console(opts?)/jsErrors(since?)/requests(opts?)/requestDetail(idxOrId,opts?)/detect()/cookies(domain?)/cookieGet(name)/cookieSet(name,value,opts?)/cookieDelete(name,domain?)/cookiesClear()/localGet(k)/localSet(k,v)/localDelete(k)/localClear()/sessionGet(k)/sessionSet(k,v)/sessionDelete(k)/sessionClear()/mouseMove(x,y)/mouseDown(button?)/mouseUp(button?)/mouseWheel(dx,dy)/hoverAt(x,y)/dropFiles(ref,paths)/highlight(ref,opts?)/highlightClear()/annotate(refs)/downloads(since?)/downloadPath(guid,s?)/emulateMedia(opts?)/emulateMediaClear()/screenshot(path?, full?)/pdf(path?)/newTab(url?)/switchTab(id)/currentTab()/closeTab(id?)/goto(url,opts?)/goBack(delta?)/goForward(delta?)/reload(opts?)/clickAt(x,y,opts?)/fillInput(sel,text,submit?)/clickRef(ref,opts?)/checkRef(ref)/uncheckRef(ref)/fillRef(ref,text,submit?)/selectOption(ref,value)/pressKey(key)/dialogStatus()/dialogAccept(text?)/dialogDismiss()/routeBlock(pattern)/routeMock(pattern,body,opts?)/routeClear()/waitLoad(s?)/waitIdle(s?)/waitForResponse(pattern,s?)/responseBody(requestId)/pageEval(js)/hoverRef(ref)/hoverAt(x,y)/dblclickRef(ref)/dragRef(src,dst)/keydown(key)/keyup(key)/typeRef(ref,text)/emulate(opts)/setInitScript(code)/exportStorageState()/importStorageState(state)/JSON.parse(string)/JSON.stringify(value,indent?)/grantPermissions(perms,origin?)/cloneCookies(domains)/recordStart(opts?)/recordChapter(title)/recordStop()/chromeInstall(opts?)/chromeList()/chromeUse(version)/chromeUpdate()/chromeRemove(version)/chromeDoctor()/print(x)";

/// 容器预览：头部 JSON 截断（留尾注位），超帽尾注总项数；小容器输出
/// 与全量形一致。不与 [`trunc_preview`] 叠用（双省略号）。
fn preview_container(kind: &str, head_json: String, head_n: usize, total: usize) -> String {
    let cap = PREVIEW_MAX_CHARS.saturating_sub(8);
    let closer = if kind == "数组" { ']' } else { '}' };
    let mut s: String = head_json.chars().take(cap).collect();
    if total > head_n {
        s.push_str(&format!("…(共 {total} 项){closer}"));
    } else if head_json.chars().count() > cap {
        s.push('…');
        s.push(closer);
    } else {
        s = head_json;
    }
    format!("{kind} {s}")
}

/// 求值值的类型化短预览（报错与 `print` 调试）：标类型加截断内容，字符串
/// 带引号、容器打 JSON 截断；杜绝「JSON 文本看着像对象」的误导（#21）。
fn preview(v: &Value) -> String {
    let v = &mask_secrets(v);
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => format!("布尔 {b}"),
        Value::Number(n) => format!("数字 {n}"),
        Value::String(s) => format!("字符串\"{}\"", trunc_preview(s)),
        Value::Array(a) => {
            let head: Vec<&Value> = a.iter().take(PREVIEW_HEAD_ITEMS).collect();
            preview_container(
                "数组",
                serde_json::to_string(&head).unwrap_or_default(),
                PREVIEW_HEAD_ITEMS,
                a.len(),
            )
        }
        Value::Object(o) => {
            let head: Map<String, Value> = o
                .iter()
                .take(PREVIEW_HEAD_ITEMS)
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            preview_container(
                "对象",
                serde_json::to_string(&head).unwrap_or_default(),
                PREVIEW_HEAD_ITEMS,
                o.len(),
            )
        }
    }
}

/// 把方言求值结果渲染成 CLI stdout 文本：字符串带引号（JSON 转义，与对象
/// 输出可区分，#21）、数字与布尔裸打、空容器不打、其余打 JSON。
///
/// # Examples
///
/// ```
/// use serde_json::json;
/// assert_eq!(browse_core::render_result(&json!("hi")), "\"hi\"");
/// assert_eq!(browse_core::render_result(&json!("a\"b\n")), "\"a\\\"b\\n\"");
/// assert_eq!(browse_core::render_result(&json!(7)), "7");
/// assert_eq!(browse_core::render_result(&json!({})), "");
/// assert_eq!(browse_core::render_result(&json!({"a": 1})), r#"{"a":1}"#);
/// ```
pub fn render_result(v: &Value) -> String {
    let v = mask_secrets(v);
    match &v {
        Value::Null => String::new(),
        Value::Array(a) if a.is_empty() => String::new(),
        Value::Object(o) if o.is_empty() => String::new(),
        other => other.to_string(),
    }
}

/// 标准字母表的 base64 解码，容忍空白，不引 crate。
///
/// # Errors
///
/// 输入含字母表与空白外的字符，或长度不是 4 的倍数（解码出无意义数据）。
///
/// # Examples
///
/// ```
/// let bytes = browse_core::js_host::base64_decode("aGk=").unwrap();
/// assert_eq!(bytes, b"hi");
/// // 空白容忍（PowerShell 折行输出形）
/// assert!(browse_core::js_host::base64_decode("aG\nk=").is_ok());
/// ```
pub fn base64_decode(s: &str) -> Result<Vec<u8>> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut val = [0u8; 256];
    for (i, b) in TABLE.iter().enumerate() {
        val[*b as usize] = i as u8;
    }
    let cleaned: Vec<u8> = s
        .bytes()
        .filter(|b| !b.is_ascii_whitespace() && *b != b'=')
        .collect();
    let mut out = Vec::with_capacity(cleaned.len() * 3 / 4);
    for chunk in cleaned.chunks(4) {
        let b = |i: usize| -> Option<u8> { chunk.get(i).map(|c| val[*c as usize]) };
        let n = ((b(0).ok_or_else(|| anyhow!("base64 非法输入"))? as u32) << 18)
            | ((b(1).ok_or_else(|| anyhow!("base64 非法输入"))? as u32) << 12)
            | (b(2).unwrap_or(0) as u32) << 6
            | b(3).unwrap_or(0) as u32;
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// glob 语义：等值、前中后缀通配、多段、锚定不误伤。
    #[test]
    fn glob_matches() {
        assert!(
            glob_match("http://a.test/x", "http://a.test/x"),
            "无 * 即等值"
        );
        assert!(!glob_match("http://a.test/x", "http://a.test/y"));
        assert!(glob_match(
            "http://mock.test/api*",
            "http://mock.test/api/data?v=1"
        ));
        assert!(glob_match(
            "*://ads.example.com/*",
            "https://ads.example.com/pixel.js"
        ));
        assert!(glob_match("http://*middle*", "http://a-middle-b"));
        assert!(
            !glob_match("http://mock.test/api*", "http://mock.test/other"),
            "前缀锚定"
        );
        assert!(!glob_match("*api", "http://mock.test/api/x"), "末段锚尾");
        assert!(glob_match("*", "anything at all"));
    }

    /// base64 编解码往返（含 padding 两态）。
    #[test]
    fn base64_roundtrip() {
        for s in ["", "a", "ab", "abc", "{\"ok\":1} 你好"] {
            let enc = base64_encode(s.as_bytes());
            let dec = base64_decode(&enc).unwrap();
            assert_eq!(dec, s.as_bytes(), "{s:?} 往返失败");
        }
    }

    /// 渲染带类型区分（#21）：字符串带引号与对象输出可区分，标量裸打不变。
    #[test]
    fn render_result_types_strings() {
        assert_eq!(render_result(&json!("hi")), "\"hi\"");
        assert_eq!(render_result(&json!("a\"b\n")), "\"a\\\"b\\n\"");
        assert_eq!(render_result(&json!(7)), "7");
        assert_eq!(render_result(&json!(true)), "true");
        assert_eq!(render_result(&json!({})), "");
        assert_eq!(render_result(&json!([])), "");
        assert_eq!(render_result(&json!({"a": 1})), r#"{"a":1}"#);
        // JSON 文本字符串不再与对象输出同形（getResponseBody 陷阱，#21）
        assert_eq!(render_result(&json!(r#"{"a":1}"#)), "\"{\\\"a\\\":1}\"");
        assert_ne!(
            render_result(&json!(r#"{"a":1}"#)),
            render_result(&json!({"a": 1}))
        );
    }

    /// 类型化预览（#21）：报错与 print 的 receiver 标类型，超长按字符截断标总长。
    #[test]
    fn preview_is_typed_and_truncated() {
        assert_eq!(preview(&json!(null)), "null");
        assert_eq!(preview(&json!(false)), "布尔 false");
        assert_eq!(preview(&json!(42)), "数字 42");
        assert_eq!(preview(&json!("abc")), "字符串\"abc\"");
        assert_eq!(preview(&json!([1, 2])), "数组 [1,2]");
        assert_eq!(preview(&json!({"a": 1})), "对象 {\"a\":1}");
        let long = "中文一二三四五六".repeat(10);
        let n = long.chars().count();
        let p = preview(&json!(long));
        assert!(p.contains(&format!("共 {n} 字符")), "超长应标注总长: {p}");
        assert!(p.chars().count() < n, "预览应被截断: {p}");
    }

    /// JSON 命名空间（#21）：parse/stringify 往返、indent 多行、错误带 CTA。
    #[tokio::test]
    async fn json_namespace_roundtrip() {
        let host = JsHost::new(cdp::Session::new());
        let v = host
            .eval_snippet(r#"return JSON.parse("{\"a\":1,\"b\":[2,3]}")"#)
            .await
            .expect("parse");
        assert_eq!(v.pointer("/b/1"), Some(&json!(3)));
        let s = host
            .eval_snippet(r#"return JSON.stringify({"a": 1})"#)
            .await
            .expect("stringify");
        assert_eq!(s, json!("{\"a\":1}"));
        let pretty = host
            .eval_snippet(r#"return JSON.stringify({"a": 1}, 2)"#)
            .await
            .expect("stringify pretty");
        assert!(
            pretty.as_str().is_some_and(|p| p.contains('\n')),
            "indent 应出多行: {pretty:?}"
        );
        let e = host
            .eval_snippet(r#"return JSON.parse("{oops")"#)
            .await
            .expect_err("坏 JSON 应报错")
            .to_string();
        assert!(e.contains("不是合法 JSON") && e.contains("下一步："), "{e}");
        let e = host
            .eval_snippet("return JSON.bogus()")
            .await
            .expect_err("未知 JSON 方法应报错")
            .to_string();
        assert!(
            e.contains("JSON.bogus 不存在") && e.contains("parse"),
            "{e}"
        );
    }

    /// 值方法面（#21）：字符串与数组方法、UTF-16 口径、错误 CTA 列方法清单。
    #[tokio::test]
    async fn value_methods_face() {
        let host = JsHost::new(cdp::Session::new());
        // 字符串：slice 正负索引与 UTF-16 单元口径、split、trim、大小写、前后缀
        let v = host
            .eval_snippet(r#"return "hello".slice(1, 3)"#)
            .await
            .unwrap();
        assert_eq!(v, json!("el"));
        let v = host
            .eval_snippet(r#"return "hello".slice(-3)"#)
            .await
            .unwrap();
        assert_eq!(v, json!("llo"));
        let v = host
            .eval_snippet(r#"return "hello".slice(3, 1)"#)
            .await
            .unwrap();
        assert_eq!(v, json!(""));
        let v = host
            .eval_snippet(r#"return "中文字".slice(1, 2)"#)
            .await
            .unwrap();
        assert_eq!(v, json!("文"));
        let v = host
            .eval_snippet(r#"return "a,b,,c".split(",")"#)
            .await
            .unwrap();
        assert_eq!(v, json!(["a", "b", "", "c"]));
        let v = host.eval_snippet(r#"return " x ".trim()"#).await.unwrap();
        assert_eq!(v, json!("x"));
        let v = host
            .eval_snippet(r#"return "Hi".toLowerCase()"#)
            .await
            .unwrap();
        assert_eq!(v, json!("hi"));
        let v = host
            .eval_snippet(r#"return "report.pdf".endsWith(".pdf")"#)
            .await
            .unwrap();
        assert_eq!(v, json!(true));
        // 数组：slice、join（null 空串、容器紧凑 JSON）、includes、concat
        let v = host
            .eval_snippet(r#"return [1,2,3,4].slice(-2)"#)
            .await
            .unwrap();
        assert_eq!(v, json!([3, 4]));
        let v = host
            .eval_snippet(r#"return [1,null,"x"].join("-")"#)
            .await
            .unwrap();
        assert_eq!(v, json!("1--x"));
        let v = host
            .eval_snippet(r#"return [{"k":1}].join()"#)
            .await
            .unwrap();
        assert_eq!(v, json!("{\"k\":1}"));
        let v = host
            .eval_snippet(r#"return [1,2].includes(2)"#)
            .await
            .unwrap();
        assert_eq!(v, json!(true));
        // includes 数值宽等（#21 评审 F）：1 与 1.0 跨形态同值，对齐 JS
        let v = host
            .eval_snippet(r#"return [1,2].includes(2.0)"#)
            .await
            .unwrap();
        assert_eq!(v, json!(true));
        let v = host
            .eval_snippet(r#"return [1.5].includes(1.5)"#)
            .await
            .unwrap();
        assert_eq!(v, json!(true));
        let v = host
            .eval_snippet(r#"return [1].includes(3)"#)
            .await
            .unwrap();
        assert_eq!(v, json!(false));
        let v = host
            .eval_snippet(r#"return [1].concat([2],[3])"#)
            .await
            .unwrap();
        assert_eq!(v, json!([1, 2, 3]));
        // 错误 CTA：类型感知，各自列方法清单
        let e = host
            .eval_snippet(r#"return "abc".nope()"#)
            .await
            .unwrap_err()
            .to_string();
        assert!(
            e.contains("字符串可调 slice") && e.contains("trim()"),
            "{e}"
        );
        let e = host
            .eval_snippet(r#"return [1].nope()"#)
            .await
            .unwrap_err()
            .to_string();
        assert!(e.contains("数组可调 slice") && e.contains("concat"), "{e}");
    }

    /// 方法面清单与 surface 目录同源绑定（#21 评审 G）：增删方法只改
    /// consts 不改目录描述，在这里红。
    #[test]
    fn method_face_catalog_lists_all_methods() {
        let entry = crate::surface::COMMANDS
            .iter()
            .find(|c| c.name == "value-methods")
            .expect("value-methods 条目应在目录");
        for m in STRING_METHODS.iter().chain(ARRAY_METHODS.iter()) {
            let name = m.split('(').next().unwrap();
            assert!(
                entry.description.contains(name),
                "{name} 应在 value-methods 目录描述里"
            );
        }
    }

    /// 全局函数 CTA 与 surface 目录同源绑定（#33 G6，双向）：增删 Global
    /// 条目不改 GLOBALS_CTA 在这里红；目录删条目后 CTA 残留同样红
    /// （value-methods 是族条目豁免）。
    #[test]
    fn global_cta_covers_catalog() {
        let globals: Vec<&str> = crate::surface::COMMANDS
            .iter()
            .filter(|c| matches!(c.kind, crate::surface::CmdKind::Global))
            .map(|c| c.name)
            .collect();
        for name in &globals {
            if *name != "value-methods" {
                assert!(GLOBALS_CTA.contains(name), "{name} 应在 GLOBALS_CTA 里");
            }
        }
        // 反向：CTA 里的每个标识符都应在目录（防删函数后提示残留）
        for entry in GLOBALS_CTA.split('/') {
            let ident = entry.split('(').next().unwrap_or("").trim();
            if ident.is_empty() {
                continue;
            }
            assert!(
                globals.contains(&ident),
                "GLOBALS_CTA 的 {ident} 不在目录 Global 条目里（删函数后提示残留）"
            );
        }
    }

    /// preview 大值有界（#33 F5）：容器只序列化头部，超帽尾注总项数。
    #[test]
    fn preview_bounds_container_serialization() {
        let big: Vec<Value> = (0..1000).map(|i| json!(i)).collect();
        let p = preview(&Value::Array(big));
        assert!(p.contains("共 1000 项"), "{p}");
        assert!(p.chars().count() < 60, "预览应短: {p}");
        // 小容器输出与全量形一致（既有测试形态不回归）
        assert_eq!(preview(&json!([1, 2])), "数组 [1,2]");
        // 对象路径：尾注按 } 收（评审 G-lite，曾错配 ]）
        let big_obj: serde_json::Map<String, Value> =
            (1..=12).map(|i| (format!("k{i}"), json!(i))).collect();
        let po = preview(&Value::Object(big_obj));
        assert!(po.contains("共 12 项)}"), "{po}");
    }

    /// 数字字面量指数/下划线即报（#33 F4）：不再静默截断或跑偏归因。
    #[test]
    fn number_literals_reject_exponent_and_underscore() {
        for src in ["return 1e5", "const x = 1_000"] {
            let err = crate::parser::parse_script(src).expect_err(src).to_string();
            assert!(
                err.contains("不支持指数或下划线"),
                "{src} 应报写法不支持: {err}"
            );
            assert!(err.contains("下一步："), "{err}");
        }
    }

    /// 集成（#18 严格测试令）：模板字符串与值方法面、JSON 命名空间、
    /// 变量表的组合流——页面代码模板在宿主侧小加工的典型链路。
    #[tokio::test]
    async fn template_composes_with_value_face_and_json() {
        let host = JsHost::new(cdp::Session::new());
        // 模板产 JSON 文本 -> JSON.parse -> 值方法加工 -> stringify 回传
        let v = host
            .eval_snippet(
                r#"const raw = `{"items": ["a", "b"]}`
return JSON.stringify(JSON.parse(raw).items.slice(0, 1))"#,
            )
            .await
            .unwrap();
        assert_eq!(v, json!(r#"["a"]"#));
        // 多行模板 + split/join 往返（模板真换行、普通串 \n 转义两口径并存）
        let v = host
            .eval_snippet("const t = `l1\nl2\nl3`\nreturn t.split(\"\\n\").join(\"-\")")
            .await
            .unwrap();
        assert_eq!(v, json!("l1-l2-l3"));
        // 模板内 ${} 字面量与反斜杠原样，length 按 UTF-16 单元
        let v = host
            .eval_snippet(r#"return `a${b}\nc`.length"#)
            .await
            .unwrap();
        assert_eq!(v, json!(8));
    }

    /// 全量 JS 旁路（#22）：eval_js 直发 Runtime.evaluate（returnByValue 加
    /// awaitPromise 双开），方言 pageEval 同能力；页内抛错带 exceptionDetails
    /// 与 CTA；缺参类型感知。
    #[cfg(unix)]
    #[tokio::test]
    async fn page_eval_js_bypass() {
        use std::io::{Read, Write};
        use std::os::unix::net::UnixStream;
        use std::sync::Mutex;
        let (sa, ba) = UnixStream::pair().unwrap();
        let (sb, bb) = UnixStream::pair().unwrap();
        let session = cdp::Session::new();
        session.connect_pipes(sa, sb).await.expect("管道连接");
        session.set_active_session(Some("S1".into())).await;
        let host = JsHost::new(session.clone());
        let seen: Arc<Mutex<Vec<(String, bool, bool)>>> = Arc::new(Mutex::new(Vec::new()));
        let peer_out = Arc::new(Mutex::new(ba));
        let (seen_p, peer_out_p) = (seen.clone(), peer_out.clone());
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
                    let expr = v
                        .pointer("/params/expression")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let rbv = v
                        .pointer("/params/returnByValue")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    let awp = v
                        .pointer("/params/awaitPromise")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    seen_p.lock().unwrap().push((expr.clone(), rbv, awp));
                    // 抛错探针：表达式带 throw 时回 exceptionDetails
                    let resp = if expr.contains("throw new Error") {
                        json!({"id": id, "result": {
                            "result": {"type": "object", "subtype": "error"},
                            "exceptionDetails": {
                                "text": "Uncaught",
                                "exception": {"description": "Error: boom\n    at <anonymous>:1:7"}
                            }
                        }})
                    } else {
                        json!({"id": id, "result": {"result": {"type": "number", "value": 42}}})
                    };
                    if let Ok(mut out) = peer_out_p.lock() {
                        let _ = out.write_all(serde_json::to_string(&resp).unwrap().as_bytes());
                        let _ = out.write_all(&[0]);
                    }
                }
            }
        });
        // 方言形态：pageEval
        let v = host
            .eval_snippet(r#"return await pageEval("1+1")"#)
            .await
            .expect("pageEval");
        assert_eq!(v, json!(42));
        // 直发形态：eval_js
        let v = host.eval_js("document.title").await.expect("eval_js");
        assert_eq!(v, json!(42));
        {
            let seen = seen.lock().unwrap();
            assert_eq!(seen.len(), 2, "两次求值各一发: {seen:?}");
            for (expr, rbv, awp) in seen.iter() {
                assert!(
                    *rbv && *awp,
                    "returnByValue 加 awaitPromise 必须双开: {expr:?} rbv={rbv} awp={awp}"
                );
            }
        }
        // 页内抛错：错误带描述与 CTA
        let e = host
            .eval_js("throw new Error('boom')")
            .await
            .expect_err("抛错应上抛");
        let msg = format!("{e:#}");
        assert!(
            msg.contains("页内抛错") && msg.contains("boom") && msg.contains("下一步"),
            "{msg}"
        );
        // 缺参/类型感知
        let e = host
            .eval_snippet("await pageEval(1)")
            .await
            .expect_err("非字符串应报 CTA");
        assert!(format!("{e:#}").contains("pageEval 的 JS 源码"), "{e:#}");
    }

    /// return 桥三洞（评审 F）加不可序列化面（评审 G）：先原样发，
    /// Illegal return 认出后包 async IIFE（换行定界）重发；值缺失区分
    /// undefined（null）与不可序列化（CTA）。
    #[cfg(unix)]
    #[tokio::test]
    async fn eval_js_return_bridge_and_unserializable() {
        use std::io::{Read, Write};
        use std::os::unix::net::UnixStream;
        use std::sync::Mutex;
        let (sa, ba) = UnixStream::pair().unwrap();
        let (sb, bb) = UnixStream::pair().unwrap();
        let session = cdp::Session::new();
        session.connect_pipes(sa, sb).await.expect("管道连接");
        session.set_active_session(Some("S1".into())).await;
        let host = JsHost::new(session.clone());
        let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let peer_out = Arc::new(Mutex::new(ba));
        let (seen_p, peer_out_p) = (seen.clone(), peer_out.clone());
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
                    let expr = v
                        .pointer("/params/expression")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    seen_p.lock().unwrap().push(expr.clone());
                    // 未包装的含 return 形 → Illegal return（模拟 V8 顶层）；
                    // 节点取值 → 无 value 的遥控对象；undefined → 无 value
                    let resp = if !expr.starts_with("(async () =>") && expr.contains("return") {
                        json!({"id": id, "result": {
                            "result": {"type": "object"},
                            "exceptionDetails": {"text": "Uncaught",
                                "exception": {"description": "SyntaxError: Illegal return statement"}}
                        }})
                    } else if expr.contains("document.body") {
                        // 真机形状（评审定谳）：returnByValue 下节点是 value:{}
                        // 无 subtype，与空对象不可区分
                        json!({"id": id, "result": {"result": {"type": "object", "value": {}}}})
                    } else if expr.contains("undefined") {
                        json!({"id": id, "result": {"result": {"type": "undefined"}}})
                    } else {
                        json!({"id": id, "result": {"result": {"type": "number", "value": 42}}})
                    };
                    if let Ok(mut out) = peer_out_p.lock() {
                        let _ = out.write_all(serde_json::to_string(&resp).unwrap().as_bytes());
                        let _ = out.write_all(&[0]);
                    }
                }
            }
        });
        // 洞一：return 不在句首（声明加末尾 return）→ 重试包装修发
        let v = host
            .eval_js("const t = 1;\nreturn t")
            .await
            .expect("声明加 return 形应经桥修过");
        assert_eq!(v, json!(42));
        {
            let seen = seen.lock().unwrap();
            assert_eq!(seen.len(), 2, "应先原样发再重发: {seen:?}");
            assert!(
                seen[1].starts_with("(async () => {"),
                "重发应是 async IIFE 包装: {:?}",
                seen[1]
            );
            assert!(
                seen[1].contains('\n'),
                "换行定界（行尾注释不吞闭合）: {:?}",
                seen[1]
            );
        }
        // 已合法的 return 开头单语句：也走原样发加重发（假对端同形），值回传
        let v = host.eval_js("return 1").await.expect("裸 return 形");
        assert_eq!(v, json!(42));
        // 不可序列化（真机形状）：value:{} 原样回传（协议层与空对象
        // 不可区分，CTA 在 surface 手册句，不硬造判据——评审定谳）
        let v = host.eval_js("document.body").await.expect("节点形");
        assert_eq!(v, json!({}), "returnByValue 下节点序列化成空对象: {v}");
        // undefined：回 null
        let v = host.eval_js("undefined").await.expect("undefined");
        assert_eq!(v, json!(null));
    }

    /// waitForResponse 体就绪等待（#20 评审 F 回归锁）：内存管道假 CDP
    /// 对端——responseReceived 先到（历史窗），getResponseBody 在
    /// loadingFinished 前被拒（No resource），函数必须等体完成信号再取；
    /// 永不完体的请求 5 秒体窗耗尽后显式 bodyError，不静默省略。
    #[cfg(unix)]
    #[tokio::test]
    async fn wait_for_response_waits_for_body_ready() {
        use std::io::{Read, Write};
        use std::os::unix::net::UnixStream;
        use std::sync::Mutex;
        let (sa, ba) = UnixStream::pair().unwrap();
        let (sb, bb) = UnixStream::pair().unwrap();
        let session = cdp::Session::new();
        session.connect_pipes(sa, sb).await.expect("管道连接");
        // F2 后 waitForResponse 只认活动 session 的事件，管道态显式设活动
        session.set_active_session(Some("S1".into())).await;
        let host = JsHost::new(session.clone());

        let released = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let peer_out = Arc::new(Mutex::new(ba));
        let (released_p, peer_out_p) = (released.clone(), peer_out.clone());
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
                    let method = v.get("method").and_then(Value::as_str).unwrap_or("");
                    let rid = v
                        .pointer("/params/requestId")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    // R2 永不放体；R1 在 released 前拒（模拟 chrome 的
                    // No resource with given identifier）
                    let resp = if method == "Network.getResponseBody"
                        && (rid == "R2" || !released_p.load(Ordering::Relaxed))
                    {
                        json!({"id": id, "error": {"code": -32000, "message": "No resource with given identifier found"}})
                    } else if method == "Network.getResponseBody" {
                        json!({"id": id, "result": {"body": "{\"ok\":\"slow\"}", "base64Encoded": false}})
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
        let send_event = |ev: Value| {
            let mut out = peer_out.lock().unwrap();
            let _ = out.write_all(serde_json::to_string(&ev).unwrap().as_bytes());
            let _ = out.write_all(&[0]);
        };
        let rr = |rid: &str, url: &str| {
            json!({
                "method": "Network.responseReceived",
                "params": {
                    "requestId": rid,
                    "response": {"url": url, "status": 200, "headers": {"Content-Type": "application/json"}}
                },
                "sessionId": "S1"
            })
        };
        send_event(rr("R1", "http://slow.test/x"));
        send_event(rr("R2", "http://stuck.test/y"));
        // F2 回归锁：他域 session 的同 URL 响应不误领（300ms 短窗超时即证）
        send_event(json!({
            "method": "Network.responseReceived",
            "params": {
                "requestId": "R9",
                "response": {"url": "http://foreign.test/z", "status": 200, "headers": {}}
            },
            "sessionId": "OTHER"
        }));
        let foreign = host
            .eval_snippet(r#"await waitForResponse("http://foreign.test/*", 1)"#)
            .await;
        assert!(
            foreign.is_err() && format!("{foreign:#?}").contains("超时"),
            "他域 session 的事件不应被命中: {foreign:?}"
        );

        // R1：400ms 后放体（loadingFinished 到），函数等到体完成再取
        let releaser = {
            let peer_out = peer_out.clone();
            let released = released.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                released.store(true, Ordering::Relaxed);
                if let Ok(mut out) = peer_out.lock() {
                    let ev = json!({"method": "Network.loadingFinished", "params": {"requestId": "R1"}, "sessionId": "S1"});
                    let _ = out.write_all(serde_json::to_string(&ev).unwrap().as_bytes());
                    let _ = out.write_all(&[0]);
                }
            })
        };
        let wr = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            host.eval_snippet(r#"return await waitForResponse("http://slow.test/*", 3)"#),
        )
        .await
        .expect("不应整体超时")
        .expect("waitForResponse R1");
        assert_eq!(
            wr.pointer("/body"),
            Some(&json!("{\"ok\":\"slow\"}")),
            "{wr}"
        );
        assert_eq!(wr.pointer("/json/ok"), Some(&json!("slow")), "{wr}");
        releaser.await.unwrap();

        // R2：体永不完，体窗耗尽后 body 为 null 加 bodyError，不静默省略
        let stuck = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            host.eval_snippet(r#"return await waitForResponse("http://stuck.test/*", 1)"#),
        )
        .await
        .expect("不应整体超时")
        .expect("waitForResponse R2");
        assert_eq!(stuck.pointer("/body"), Some(&Value::Null), "{stuck}");
        assert!(
            stuck
                .pointer("/bodyError")
                .and_then(Value::as_str)
                .is_some_and(|e| e.contains("No resource")),
            "bodyError 应带因: {stuck}"
        );
    }

    /// 快照消注入痕（#30）：snapshot 零 Runtime.evaluate（对端方法面留痕
    /// 即证）、url/title 走 CDP 查询面、文档代计数按事件谱精确递增
    /// （主框架导航 +1，iframe 与同文档跳转不动）、导航/换靶后旧 ref
    /// 给重取 CTA。
    #[cfg(unix)]
    #[tokio::test]
    async fn snapshot_is_injection_free_and_generation_counted() {
        use std::io::{Read, Write};
        use std::os::unix::net::UnixStream;
        use std::sync::Mutex;
        let (sa, ba) = UnixStream::pair().unwrap();
        let (sb, bb) = UnixStream::pair().unwrap();
        let session = cdp::Session::new();
        session.connect_pipes(sa, sb).await.expect("管道连接");
        session.set_active_session(Some("S1".into())).await;
        let host = JsHost::new(session.clone());

        let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let peer_out = Arc::new(Mutex::new(ba));
        let (seen_p, peer_out_p) = (seen.clone(), peer_out.clone());
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
                    let method = v
                        .get("method")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    seen_p.lock().unwrap().push(method.clone());
                    let resp = match method.as_str() {
                        "Accessibility.getFullAXTree" => json!({"id": id, "result": {
                            "nodes": [
                                {"nodeId": 1, "role": {"value": "button"}, "name": {"value": "Go"},
                                 "backendDOMNodeId": 42}
                            ]
                        }}),
                        // navigate 回执带 loaderId（跨文档形态，#57 F3）：
                        // 同文档 fragment 回执无 loaderId 不换代
                        "Page.navigate" => json!({"id": id, "result": {
                            "loaderId": "L1", "frameId": "F1"
                        }}),
                        // active_target 未设（管道态），page_meta 回落导航史
                        "Page.getNavigationHistory" => json!({"id": id, "result": {
                            "currentIndex": 0,
                            "entries": [{"id": 1, "url": "http://h.test/", "title": "HT"}]
                        }}),
                        _ => json!({"id": id, "result": {}}),
                    };
                    if let Ok(mut out) = peer_out_p.lock() {
                        let _ = out.write_all(serde_json::to_string(&resp).unwrap().as_bytes());
                        let _ = out.write_all(&[0]);
                    }
                }
            }
        });
        let send_event = |ev: Value| {
            let mut out = peer_out.lock().unwrap();
            let _ = out.write_all(serde_json::to_string(&ev).unwrap().as_bytes());
            let _ = out.write_all(&[0]);
        };

        // snapshot：url/title 来自 CDP 查询面，节点带 ref
        let snap = host
            .eval_snippet("return await snapshot()")
            .await
            .expect("snapshot");
        assert_eq!(
            snap.pointer("/url"),
            Some(&json!("http://h.test/")),
            "{snap}"
        );
        assert_eq!(snap.pointer("/title"), Some(&json!("HT")), "{snap}");
        assert_eq!(snap.pointer("/nodes/0/ref"), Some(&json!("e1")), "{snap}");
        // 零 evaluate：snapshot 链路不许再碰 Runtime.evaluate（含代际标记）
        {
            let methods = seen.lock().unwrap().clone();
            assert!(
                !methods.iter().any(|m| m == "Runtime.evaluate"),
                "snapshot 不应求值页面（#30 消注入痕），实际调用: {methods:?}"
            );
        }

        // call() 同步计数（评审 G4）：Page.navigate 回执即加代，不等事件
        let nav = host
            .eval_snippet(r#"await session.Page.navigate({url:"http://h.test/x"})"#)
            .await;
        assert!(nav.is_ok(), "假对端应答 navigate: {nav:?}");
        assert_eq!(
            session.doc_generation("S1").await,
            1,
            "navigate 回执后应立即加代（不等事件）"
        );

        // 文档代事件谱：主框架导航 +1；iframe（有 parentId）与同文档跳转不动
        send_event(json!({"method": "Page.navigatedWithinDocument",
            "params": {"url": "http://h.test/#x", "frameId": "F1"}, "sessionId": "S1"}));
        send_event(json!({"method": "Page.frameNavigated",
            "params": {"frame": {"id": "F2", "parentId": "F1"}}, "sessionId": "S1"}));
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        assert_eq!(
            session.doc_generation("S1").await,
            1,
            "同文档跳转与 iframe 导航不应递增文档代"
        );
        send_event(json!({"method": "Page.frameNavigated",
            "params": {"frame": {"id": "F1"}}, "sessionId": "S1"}));
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        assert_eq!(
            session.doc_generation("S1").await,
            2,
            "主框架导航应递增文档代"
        );

        // 导航后旧 ref：重取 CTA（不静默点错位置）
        let stale = host.eval_snippet(r#"await clickRef("e1")"#).await;
        let msg = format!("{stale:#?}");
        assert!(
            stale.is_err() && msg.contains("已过期") && msg.contains("snapshot"),
            "导航后旧 ref 应失效并带重取 CTA: {msg}"
        );
        // 换 tab 后同样作废（backendNodeId 跨 target 无意义，代恰好同值也不可信）
        session.set_active_session(Some("S2".into())).await;
        let stale = host.eval_snippet(r#"await clickRef("e1")"#).await;
        let msg = format!("{stale:#?}");
        assert!(
            stale.is_err() && msg.contains("换过 tab"),
            "引用非快照 session 应作废（代恰好同值也不可信）: {msg}"
        );
    }

    /// 提交窗重试语义（#34 评审 G1）：先两次 Not attached 再成功（重试
    /// 救回）；非瞬时错误立刻上抛（不吞不重试）。contains 判据写宽写窄
    /// 都在这里红。
    #[cfg(unix)]
    #[tokio::test]
    async fn commit_window_retry_semantics() {
        use std::io::{Read, Write};
        use std::os::unix::net::UnixStream;
        use std::sync::Mutex;
        let (sa, ba) = UnixStream::pair().unwrap();
        let (sb, bb) = UnixStream::pair().unwrap();
        let session = cdp::Session::new();
        session.connect_pipes(sa, sb).await.expect("管道连接");
        session.set_active_session(Some("S1".into())).await;
        // 不建 JsHost（watcher 噪声），重试逻辑挂在宿主方法上，经
        // eval_snippet 走 screenshot 出口驱动
        let host = JsHost::new(session.clone());

        let state = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let peer_out = Arc::new(Mutex::new(ba));
        let (state_p, peer_out_p) = (state.clone(), peer_out.clone());
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
                    let method = v.get("method").and_then(Value::as_str).unwrap_or("");
                    let resp = if method == "Page.captureScreenshot" {
                        // 前两次拒绝（提交窗），第三次出图
                        let n = state_p.fetch_add(1, Ordering::Relaxed);
                        if n < 2 {
                            json!({"id": id, "error": {"code": -32000,
                                "message": "Not attached to an active page"}})
                        } else if n == 2 {
                            // 1x1 PNG 的 base64
                            json!({"id": id, "result": {"data":
                                "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=="}})
                        } else {
                            // 之后的调用（第二次 screenshot）：别的错误，应立刻上抛
                            json!({"id": id, "error": {"code": -32000,
                                "message": "Compositing happens before... something else"}})
                        }
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
        // 第一次：两次 Not attached 后第三次成功（重试救回）
        let tmp = std::env::temp_dir().join("browse-commit-retry-test.png");
        let _ = std::fs::remove_file(&tmp);
        let shot = host
            .eval_snippet(r#"return await screenshot("/tmp/browse-commit-retry-test.png")"#)
            .await
            .expect("重试后应成功");
        assert_eq!(
            shot.get("skipped"),
            Some(&json!(false)),
            "两次瞬时拒绝后第三次应出图: {shot}"
        );
        // 第二次：非瞬时错误立刻上抛，错误原文不吞
        let e = host
            .eval_snippet(r#"return await screenshot("/tmp/browse-commit-retry-test.png")"#)
            .await
            .expect_err("非瞬时错误应上抛");
        let msg = format!("{e:#}");
        assert!(
            msg.contains("something else") && !msg.contains("重试 2 秒"),
            "别的错误不该进重试也不该带耗尽提示: {msg}"
        );
        let _ = std::fs::remove_file(&tmp);
    }

    /// 提交屏障（#34 根因级）：跨文档 navigate（回执带 loaderId）后首个
    /// 页面级调用等主框架 frameNavigated 再放行；同文档导航（无 loaderId）
    /// 不设屏障；屏障满足后只等一次；超时放行不报错；detach 清 session
    /// 记账（批 10 遗留）。
    #[cfg(unix)]
    #[tokio::test]
    async fn commit_barrier_gates_navigations() {
        use std::io::{Read, Write};
        use std::os::unix::net::UnixStream;
        use std::sync::Mutex;
        let (sa, ba) = UnixStream::pair().unwrap();
        let (sb, bb) = UnixStream::pair().unwrap();
        let session = cdp::Session::new();
        session.connect_pipes(sa, sb).await.expect("管道连接");
        session.set_active_session(Some("S1".into())).await;
        let host = JsHost::new(session.clone());

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
                    let url = v
                        .pointer("/params/url")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    // 片段导航（#x）无 loaderId；跨文档带
                    let resp = if v.get("method").and_then(Value::as_str) == Some("Page.navigate")
                        && !url.contains('#')
                    {
                        json!({"id": id, "result": {
                            "frameId": "F1", "loaderId": "L1", "isDownload": false
                        }})
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
        let commit_after = |ms: u64| {
            let peer_out = peer_out.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
                if let Ok(mut out) = peer_out.lock() {
                    let ev = json!({"method": "Page.frameNavigated",
                        "params": {"frame": {"id": "F1"}}, "sessionId": "S1"});
                    let _ = out.write_all(serde_json::to_string(&ev).unwrap().as_bytes());
                    let _ = out.write_all(&[0]);
                }
            })
        };

        // 长会话回归锁（评审 F）：预灌 60 条旧主框架导航事件（seq 小于
        // 水位），屏障必须仍被 300ms 后的新 commit 满足——首版 peek 取
        // 最旧 50 条在此永久出窗、白等满窗（生产 5 秒/导航）
        for i in 0..60 {
            let mut out = peer_out.lock().unwrap();
            let ev = json!({"method": "Page.frameNavigated",
                "params": {"frame": {"id": "F1"}}, "sessionId": "S1", "pad": i});
            let _ = out.write_all(serde_json::to_string(&ev).unwrap().as_bytes());
            let _ = out.write_all(&[0]);
        }
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;

        // 跨文档导航：Page.enable 被闸到 300ms 后的 commit 事件才放行
        let releaser = commit_after(300);
        let t0 = tokio::time::Instant::now();
        host.eval_snippet(
            r#"await session.Page.navigate({url:"http://cross.test/a"}); await session.Page.enable({})"#,
        )
        .await
        .expect("跨文档导航后页面调用");
        let gated = t0.elapsed();
        releaser.await.unwrap();
        assert!(
            gated >= std::time::Duration::from_millis(250),
            "屏障应等到 commit 事件（实测 {gated:?}）"
        );
        assert!(
            gated < std::time::Duration::from_millis(2_500),
            "长缓冲下屏障不应退化为满窗等待（实测 {gated:?}；旧 peek 取最旧 50 条时此格红）"
        );

        // 屏障满足后只等一次：紧接着的调用不再等
        let t0 = tokio::time::Instant::now();
        host.eval_snippet(r#"await session.Page.enable({})"#)
            .await
            .expect("第二次页面调用");
        assert!(
            t0.elapsed() < std::time::Duration::from_millis(200),
            "屏障清水位后不应再等（实测 {:?}）",
            t0.elapsed()
        );

        // 同文档导航（fragment）：无 loaderId 不设屏障，立即放行
        let t0 = tokio::time::Instant::now();
        host.eval_snippet(
            r#"await session.Page.navigate({url:"http://cross.test/a#frag"}); await session.Page.enable({})"#,
        )
        .await
        .expect("同文档导航后页面调用");
        assert!(
            t0.elapsed() < std::time::Duration::from_millis(300),
            "fragment 导航不应设屏障（实测 {:?}）",
            t0.elapsed()
        );
    }

    /// 换靶 detach 清 session 记账（批 10 遗留）：doc_gens 与提交屏障不随
    /// 陈旧 sid 累积；重附得新 sid 代从 0 起（旧 ref 本就因 sid 变化作废）。
    #[cfg(unix)]
    #[tokio::test]
    async fn detach_cleans_session_accounting() {
        use std::io::{Read, Write};
        use std::os::unix::net::UnixStream;
        use std::sync::Mutex;
        let (sa, ba) = UnixStream::pair().unwrap();
        let (sb, bb) = UnixStream::pair().unwrap();
        let session = cdp::Session::new();
        session.connect_pipes(sa, sb).await.expect("管道连接");
        // attach/detach 都由 use_target 发，假对端按调用序发新 sessionId
        let attach_n = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let seen_detach = Arc::new(Mutex::new(Vec::<String>::new()));
        let peer_out = Arc::new(Mutex::new(ba));
        let (attach_p, detach_p, peer_out_p) =
            (attach_n.clone(), seen_detach.clone(), peer_out.clone());
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
                    let method = v.get("method").and_then(Value::as_str).unwrap_or("");
                    let resp = match method {
                        "Target.attachToTarget" => {
                            let n = attach_p.fetch_add(1, Ordering::Relaxed);
                            json!({"id": id, "result": {"sessionId": format!("S{}", n + 1)}})
                        }
                        "Target.detachFromTarget" => {
                            let sid = v
                                .pointer("/params/sessionId")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string();
                            detach_p.lock().unwrap().push(sid);
                            json!({"id": id, "result": {}})
                        }
                        "Page.navigate" => {
                            json!({"id": id, "result": {"frameId": "F1", "loaderId": "L1"}})
                        }
                        _ => json!({"id": id, "result": {}}),
                    };
                    if let Ok(mut out) = peer_out_p.lock() {
                        let _ = out.write_all(serde_json::to_string(&resp).unwrap().as_bytes());
                        let _ = out.write_all(&[0]);
                    }
                }
            }
        });
        let send_event = |ev: Value| {
            let mut out = peer_out.lock().unwrap();
            let _ = out.write_all(serde_json::to_string(&ev).unwrap().as_bytes());
            let _ = out.write_all(&[0]);
        };

        let s1 = session.use_target("T1").await.expect("attach T1");
        assert_eq!(s1, "S1");
        // S1 上跑一次跨文档导航（同步计数 1 加屏障水位入账）
        session
            .call("Page.navigate", json!({ "url": "http://a.test/" }))
            .await
            .expect("navigate");
        send_event(json!({"method": "Page.frameNavigated",
            "params": {"frame": {"id": "F1"}}, "sessionId": "S1"}));
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        assert_eq!(session.doc_generation("S1").await, 2, "同步加事件双计数");

        // 换靶：detach S1，记账随清
        let s2 = session.use_target("T2").await.expect("attach T2");
        assert_eq!(s2, "S2");
        assert_eq!(
            session.doc_generation("S1").await,
            0,
            "detach 后旧 sid 记账应清"
        );
        assert!(
            seen_detach.lock().unwrap().iter().any(|s| s == "S1"),
            "detachFromTarget 应已发出"
        );
    }

    /// 密钥命名空间与脱敏（#25.4）：dotenv 加载、secrets.<NAME> 取值、
    /// 渲染与报错回显面具；值含密钥即整值换 ***（保守全换）。
    #[tokio::test]
    async fn secrets_namespace_and_masking() {
        let _ser = SECRETS_TEST_LOCK.lock().await;
        // 直接装进程级密钥仓（绕文件，测行为面）
        {
            let mut g = SECRETS.lock().unwrap();
            *g = vec![
                ("API_KEY".into(), "sk-super-secret".into()),
                ("EMPTY".into(), String::new()),
            ];
        }
        let host = JsHost::new(cdp::Session::new());
        let v = host
            .eval_snippet(r#"return secrets.API_KEY"#)
            .await
            .expect("取密钥");
        assert_eq!(
            v,
            json!("sk-super-secret"),
            "方言值是真值（面具只在渲染面）"
        );
        // 渲染面：裸值、嵌在对象里、报错回显，全部脱敏（字符串带引号形，批 2 类型口径）
        assert_eq!(render_result(&v), "\"***\"");
        let nested = json!({ "token": "sk-super-secret", "n": 1, "arr": ["x", "sk-super-secret"] });
        let r = render_result(&nested);
        assert!(!r.contains("sk-super-secret"), "{r}");
        assert!(r.contains(r#""token":"***""#), "{r}");
        let e = host
            .eval_snippet(r#"return "tok sk-super-secret".nope()"#)
            .await
            .unwrap_err()
            .to_string();
        assert!(!e.contains("sk-super-secret"), "报错回显应脱敏: {e}");
        // 未加载键：CTA
        let e = host
            .eval_snippet("return secrets.NOPE")
            .await
            .unwrap_err()
            .to_string();
        assert!(e.contains("secrets.NOPE") && e.contains("下一步"), "{e}");
        // 错误串子串脱敏（评审 G4）：密钥片段被换、CTA 保留
        let es = mask_secrets_str("前缀 sk-super-secret 后缀；下一步：照抄");
        assert!(
            es.contains("***") && !es.contains("sk-super-secret"),
            "{es}"
        );
        assert!(es.contains("下一步：照抄"), "{es}");
        // 清仓后不再脱敏（幂等装载语义）
        {
            let mut g = SECRETS.lock().unwrap();
            g.clear();
        }
        assert_eq!(
            render_result(&json!("sk-super-secret")),
            "\"sk-super-secret\""
        );
    }

    /// dotenv 解析（#25.4）：注释空行、引号剥离、重复键覆盖、坏行报行号。
    #[tokio::test]
    async fn secrets_dotenv_parsing() {
        let _ser = SECRETS_TEST_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("browse-sec-{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok();
        let f = dir.join("s.env");
        std::fs::write(
            &f,
            "# 注释\nA=1\n\nB=\"two words\"\nC='sq'\nA=2\nexport D=exp\nE=bare # trailing\nF=\"quoted # kept\"\nG=abc#nospace\nexport  H=two-space\nexport\tI=tab\nEXPORTER=keep\n",
        )
        .unwrap();
        load_secrets(f.to_str().unwrap()).expect("解析");
        {
            let g = SECRETS.lock().unwrap();
            let get = |k: &str| {
                g.iter()
                    .find(|(ek, _)| ek == k)
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default()
            };
            assert_eq!(get("A"), "2", "重复键后者覆盖");
            assert_eq!(get("B"), "two words");
            assert_eq!(get("C"), "sq");
            // 批 9 G2：export 前缀与行内注释
            assert_eq!(get("D"), "exp", "export 前缀应剥");
            assert_eq!(get("E"), "bare", "未引值行内注释应截断");
            assert_eq!(get("F"), "quoted # kept", "引号内 # 是字面");
            assert_eq!(get("G"), "abc#nospace", "# 前无空格是字面（dotenv 通例）");
            assert_eq!(get("H"), "two-space", "export 加多空格前缀应剥");
            assert_eq!(get("I"), "tab", "export 后制表符分隔也应剥（评审追加固）");
            assert_eq!(get("EXPORTER"), "keep", "exporter 键不被误剥前缀");
        }
        let bad = dir.join("bad.env");
        std::fs::write(&bad, "no-equal-line\n").unwrap();
        let e = load_secrets(bad.to_str().unwrap()).unwrap_err().to_string();
        assert!(e.contains("第 1 行") && e.contains("下一步"), "{e}");
        // BOM 剥离（评审 G1）：带 BOM 的首键可查
        let bom = dir.join("bom.env");
        std::fs::write(&bom, "\u{feff}BOMKEY=boom\n").unwrap();
        load_secrets(bom.to_str().unwrap()).expect("BOM 解析");
        {
            let g = SECRETS.lock().unwrap();
            assert!(
                g.iter().any(|(k, v)| k == "BOMKEY" && v == "boom"),
                "BOM 应被剥: {:?}",
                g
            );
        }
        // 清仓防串测
        SECRETS.lock().unwrap().clear();
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 模板字符串求值（#18）：raw 语义直出，反斜杠与真换行原样。
    #[tokio::test]
    async fn template_string_evals_raw() {
        let host = JsHost::new(cdp::Session::new());
        let v = host
            .eval_snippet("return `line1\nline2 \\d`")
            .await
            .unwrap();
        assert_eq!(v, json!("line1\nline2 \\d"));
        // 评审 F 复现件：偶数反斜杠后照常闭合，尾注释照剥不炸 token
        let v = host.eval_snippet("return `a\\\\` // 注").await.unwrap();
        assert_eq!(v, json!("a\\\\"));
    }

    /// length 成员访问：字符串（UTF-16 单元）与数组（元素数）求值不静默（#15 回归锁）。
    #[tokio::test]
    async fn length_member_returns_value() {
        let host = JsHost::new(cdp::Session::new());
        let v = host
            .eval_snippet(r#"return "abc".length"#)
            .await
            .expect("求值应成功");
        assert_eq!(v, json!(3));
        let v = host
            .eval_snippet(r#"const t = [1, 2]; return t.length"#)
            .await
            .expect("数组 length 应可求值");
        assert_eq!(v, json!(2));
        // JS 语义对齐：BMP 外字符按 UTF-16 单元计（一个 emoji 占 2）
        let v = host
            .eval_snippet("return \"😀\".length")
            .await
            .expect("emoji length 应可求值");
        assert_eq!(v, json!(2));
    }
}

#[cfg(test)]
mod timeout_tests {
    use super::*;

    /// #51 秒口径混用守卫：直写秒原样放大；大于 3600 按毫秒误写换算并告警，
    /// 换算后封顶 600 秒。
    #[test]
    fn secs_to_ms_units() {
        // 秒口径直写：15 秒即 15000ms，旧习惯 waitLoad(15000) 经守卫等价
        assert_eq!(secs_to_ms(15), (15_000, None));
        assert_eq!(secs_to_ms(0), (0, None));
        // 判据收紧（#57 F1）：999 秒是最后一位合法秒直写，1000 起即误写
        assert_eq!(secs_to_ms(999), (999_000, None));
        assert_eq!(secs_to_ms(1000).0, 1_000);
        assert!(secs_to_ms(1000).1.is_some());
        assert_eq!(secs_to_ms(3600).0, 3_000);
        // 毫秒误写：15000 换算 15 秒并告警（与旧语义同效，迁移零破坏）；
        // 3000 也已是误写（判据收紧后 1000..3600 窗不再静默秒直解）
        assert_eq!(secs_to_ms(3000).0, 3_000);
        assert!(secs_to_ms(3000).1.is_some());
        let (ms, warn) = secs_to_ms(15_000);
        assert_eq!(ms, 15_000);
        assert!(warn.is_some_and(|w| w.contains("毫秒误写")));
        // 封顶：600000（600 秒的毫秒写法）换算后恰 600 秒；再大也钳 600
        assert_eq!(secs_to_ms(600_000).0, 600_000);
        assert_eq!(secs_to_ms(7_200_000).0, 600_000);
        // 边界随判据收紧（#57 F1）：999/1000 是新界，3600 已是误写路径
        assert!(secs_to_ms(999).1.is_none());
    }

    /// 缺省走 default_s；实参位次取值。
    #[test]
    fn timeout_ms_of_positional() {
        let argv = vec![serde_json::json!("x"), serde_json::json!(5)];
        assert_eq!(
            timeout_ms_of(&argv, 1, 10, "waitLoad").unwrap(),
            (5_000, None)
        );
        assert_eq!(
            timeout_ms_of(&argv, 9, 10, "waitLoad").unwrap(),
            (10_000, None)
        );
    }

    /// 实参在位但非整数秒形态即报错（评审 G4：不静默落缺省）。
    #[test]
    fn timeout_arg_bad_type_errors() {
        let argv = vec![serde_json::json!("x"), serde_json::json!("30s")];
        assert!(timeout_ms_of(&argv, 1, 10, "waitLoad").is_err());
        let argv = vec![serde_json::json!(1.5)];
        assert!(timeout_ms_of(&argv, 0, 10, "waitJs").is_err());
        // null 在位视同缺省（方言可显式传 undefined）
        let argv = vec![serde_json::json!(Value::Null)];
        assert_eq!(
            timeout_ms_of(&argv, 0, 10, "waitLoad").unwrap(),
            (10_000, None)
        );
    }

    /// fill 类第三参 submit 两形：布尔 true 与对象 {submit: true}。
    #[test]
    fn submit_arg_shapes() {
        assert!(submit_arg(Some(&serde_json::json!(true))));
        assert!(submit_arg(Some(&serde_json::json!({"submit": true}))));
        assert!(!submit_arg(Some(&serde_json::json!(false))));
        assert!(!submit_arg(Some(&serde_json::json!({"submit": false}))));
        assert!(!submit_arg(None));
    }
}
