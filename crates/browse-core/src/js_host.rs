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
    /// 连同页窗口的代标记（主动代际失效）。整表随 snapshot 替换。
    refs: Mutex<Option<RefTable>>,
    /// 进行中的录制（至多一场；方言面 recordStart/recordStop 管理）。
    record: Mutex<Option<crate::record::Recorder>>,
    /// 网络拦截规则（routeBlock/routeMock 管理，watcher 应答 requestPaused）。
    routes: Arc<Mutex<Vec<RouteRule>>>,
    /// Fetch 域是否由本宿主开启（手动 `session.Fetch.enable` 不被 watcher 打扰）。
    fetch_by_us: Arc<std::sync::atomic::AtomicBool>,
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
    },
}

/// 引用表：短 ref -> backendNodeId，加 snapshot 时刻的页窗口代标记。
/// 代标记不匹配（文档被导航重开）即整表作废；SPA 同文档跳转（pushState）
/// 不重开窗口、代不变，ref 继续有效；比 URL 对比零假阳性。
#[derive(Clone)]
struct RefTable {
    generation: i64,
    map: HashMap<String, i64>,
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
        spawn_dialog_watcher(session.clone());
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
                let path = argv.first().and_then(Value::as_str).map(str::to_string);
                let full = argv.get(1).and_then(Value::as_bool).unwrap_or(false);
                let mut params = json!({ "format": "png" });
                if full {
                    params["captureBeyondViewport"] = json!(true);
                }
                let r = self.session.call("Page.captureScreenshot", params).await?;
                let data = r
                    .get("data")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow!("captureScreenshot 未回 data"))?;
                let bytes = base64_decode(data)?;
                let path = match path {
                    Some(p) => std::path::PathBuf::from(p),
                    None => {
                        let dir = crate::paths::state_dir().join("screenshots");
                        tokio::fs::create_dir_all(&dir).await.ok();
                        let ts = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis())
                            .unwrap_or(0);
                        dir.join(format!("shot-{ts}.png"))
                    }
                };
                let display = path.display().to_string();
                tokio::fs::write(&path, &bytes).await?;
                Ok(json!({ "path": display, "bytes": bytes.len() }))
            }
            // AX 树快照（对齐 browser-use-pi 的 snapshot 原语）：
            // getFullAXTree -> 精简节点表；url/title 一并带回
            "snapshot" => {
                let r = self
                    .session
                    .call("Accessibility.getFullAXTree", json!({}))
                    .await?;
                let info = self
                    .session
                    .call(
                        "Runtime.evaluate",
                        json!({
                            "expression": "JSON.stringify([location.href, document.title, (() => { window.__browse_ref_gen = (window.__browse_ref_gen || 0) + 1; return window.__browse_ref_gen; })()])",
                            "returnByValue": true
                        }),
                    )
                    .await?;
                let (url, title, generation) = info
                    .pointer("/result/value")
                    .and_then(Value::as_str)
                    .and_then(|s| serde_json::from_str::<Vec<Value>>(s).ok())
                    .map(|v| {
                        (
                            v.first()
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                            v.get(1)
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                            v.get(2).and_then(Value::as_i64).unwrap_or(0),
                        )
                    })
                    .unwrap_or_default();
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
                        // 丢纯布局节点：无名的 generic/文本框/展示层
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
                        Some(json!({
                            "id": n.get("nodeId"),
                            "role": role,
                            "name": name,
                            "value": n.get("value").and_then(|v| v.get("value")),
                            "checked": n.get("checked"),
                            "pressed": n.get("pressed"),
                            "selected": n.get("selected"),
                            "expanded": n.get("expanded"),
                            "disabled": n.get("disabled"),
                            "backendNodeId": n.get("backendDOMNodeId"),
                        }))
                    })
                    .collect();
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
                        refmap.insert(r, bn);
                        Some(n)
                    })
                    .collect();
                *self.refs.lock().await = Some(RefTable {
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
                self.assert_no_dialog().await?;
                crate::semantic::click_at(&self.session, x, y).await
            }
            "fillInput" => {
                let sel = argv.first().and_then(Value::as_str).ok_or_else(|| anyhow!(
                    "fillInput 缺选择器；下一步：fillInput(\"#q\", \"hello\")（CSS 选择器，填完回读验证）"
                ))?;
                let text = argv.get(1).and_then(Value::as_str).unwrap_or("");
                self.assert_no_dialog().await?;
                crate::semantic::fill_input(&self.session, sel, text).await
            }
            "pressKey" => {
                let key = argv.first().and_then(Value::as_str).ok_or_else(|| anyhow!(
                    "pressKey 缺键名；下一步：pressKey(\"Enter\") / pressKey(\"Tab\") / pressKey(\"a\")"
                ))?;
                self.assert_no_dialog().await?;
                crate::semantic::press_key(&self.session, key).await
            }
            "waitLoad" => {
                let ms = argv.first().and_then(Value::as_u64).unwrap_or(10_000);
                crate::semantic::wait_load(&self.session, ms).await
            }
            "waitIdle" => {
                let ms = argv.first().and_then(Value::as_u64).unwrap_or(10_000);
                crate::semantic::wait_idle(&self.session, ms).await
            }
            // ---- 元素引用（D35-lite）：ref 来自最近一次 snapshot() ----
            "clickRef" => {
                let r = argv.first().and_then(Value::as_str).ok_or_else(|| anyhow!(
                    "clickRef 缺 ref；下一步：clickRef(\"e3\")，ref 在最近一次 snapshot() 返回的 nodes[].ref"
                ))?;
                self.assert_no_dialog().await?;
                let bn = self.lookup_ref(r).await?;
                crate::semantic::click_ref(&self.session, bn).await
            }
            "fillRef" => {
                let r = argv.first().and_then(Value::as_str).ok_or_else(|| anyhow!(
                    "fillRef 缺 ref；下一步：fillRef(\"e2\", \"hello\")，ref 在最近一次 snapshot() 返回的 nodes[].ref"
                ))?;
                let text = argv.get(1).and_then(Value::as_str).unwrap_or("");
                self.assert_no_dialog().await?;
                let bn = self.lookup_ref(r).await?;
                crate::semantic::fill_ref(&self.session, bn, text).await
            }
            "selectOption" => {
                let r = argv.first().and_then(Value::as_str).ok_or_else(|| anyhow!(
                    "selectOption 缺 ref；下一步：selectOption(\"e4\", \"Beta\")（value 或可见 label，ref 来自 snapshot()）"
                ))?;
                let value = argv.get(1).and_then(Value::as_str).ok_or_else(|| anyhow!(
                    "selectOption 缺选项值；下一步：selectOption(\"e4\", \"Beta\")（第二参是 value 或可见 label）"
                ))?;
                self.assert_no_dialog().await?;
                let bn = self.lookup_ref(r).await?;
                crate::semantic::select_option(&self.session, bn, value).await
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
                let brief = r.brief();
                *rec = Some(r);
                Ok(brief)
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
                crate::record::stop(&self.session, rec).await
            }
            other => bail!(
                "未知函数 {other}；下一步：可用全局 listPageTargets()/resolveWsUrl()/detectBrowsers()/cdpMethods(domain?)/snapshot()/screenshot(path?, full?)/pdf(path?)/newTab(url?)/switchTab(id)/currentTab()/closeTab(id?)/clickAt(x,y)/fillInput(sel,text)/clickRef(ref)/fillRef(ref,text)/selectOption(ref,value)/pressKey(key)/dialogStatus()/dialogAccept(text?)/dialogDismiss()/routeBlock(pattern)/routeMock(pattern,body,opts?)/routeClear()/waitLoad(ms?)/waitIdle(ms?)/recordStart(opts?)/recordStop()/chromeInstall(opts?)/chromeList()/chromeUse(version)/chromeUpdate()/chromeRemove(version)/chromeDoctor()/print(x)；CDP 走 session.<Domain>.<method>(params)"
            ),
        }
    }

    /// 查短 ref 对应的 backendNodeId（只认最近一次 snapshot 的表），
    /// 并做主动代际校验。
    ///
    /// snapshot 时在页窗口盖过 `__browse_ref_gen` 代标记，引用前核对：
    /// 不匹配即文档已被导航重开，整表作废，给重取 CTA。标记取不到
    /// （evaluate 失败）不拦，退给被动失效（resolveNode/零尺寸）。
    async fn lookup_ref(&self, r: &str) -> Result<i64> {
        let table = self.refs.lock().await.clone();
        let Some(t) = table else {
            bail!(
                "还没有元素引用（引用表来自 snapshot()）；下一步：先 await snapshot()，用返回里 nodes[].ref"
            );
        };
        let Some(bn) = t.map.get(r).copied() else {
            bail!(
                "未知 ref {r}（引用表只保留最近一次 snapshot()）；下一步：先 await snapshot()，用返回里 nodes[].ref"
            );
        };
        // 求值成功但代标记对不上（含 undefined=文档已被导航重开）：主动判整表作废；
        // 求值失败（标记取不到）不拦，退给被动兜底（resolveNode/零尺寸）
        if let Ok(resp) = self
            .session
            .call(
                "Runtime.evaluate",
                json!({ "expression": "window.__browse_ref_gen", "returnByValue": true }),
            )
            .await
            && resp.pointer("/result/value").and_then(Value::as_i64) != Some(t.generation)
        {
            bail!(
                "ref 已过期（页面文档已换代，旧 ref 全体作废）；下一步：重新 await snapshot() 取新 ref"
            );
        }
        Ok(bn)
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
                Ok(json!(sid))
            }
            "waitFor" | "wait_for" => {
                let ev = argv
                    .first()
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow!(
                        "waitFor 缺 method 字符串；下一步：await session.waitFor(\"Page.loadEventFired\", undefined, 15000)"
                    ))?;
                let ms = argv
                    .get(2)
                    .and_then(Value::as_u64)
                    .or_else(|| argv.get(1).and_then(Value::as_u64))
                    .unwrap_or(15_000);
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
                        "waitJs 缺 expression 字符串；下一步：await session.waitJs(\"document.querySelector('#x') !== null\", 5000)（页内表达式真值即通过，毫秒）"
                    ))?;
                let timeout_ms = argv.get(1).and_then(Value::as_u64).unwrap_or(10_000);
                let deadline = tokio::time::Instant::now()
                    + std::time::Duration::from_millis(timeout_ms.min(120_000));
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
                            "waitJs 超时（{}ms）：{expr}{}",
                            timeout_ms,
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
                "未知 session.{other}；下一步：宿主面 connect/close/use/setActiveSession/waitFor/call/peekEvents/peekEventsSince/findEvents/isConnected/getActiveSession；CDP 域写 session.<Domain>.<method>(params)"
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
fn spawn_dialog_watcher(session: Arc<Session>) {
    tokio::spawn(async move {
        let auto =
            !std::env::var_os("BROWSE_NO_AUTO_DIALOG").is_some_and(|v| v == "1" || v == "true");
        let mut enabled: HashSet<String> = HashSet::new();
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            // 活动路由换 session 后补开域（幂等）
            if let Some(sid) = session.get_active_session().await
                && !enabled.contains(&sid)
                && session
                    .call_on("Page.enable", json!({}), &sid)
                    .await
                    .is_ok()
            {
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
                    }) => {
                        // mock 是测试原语：默认带 ACAO，跨源页（data: 测试页）也读得到
                        let _ = session
                            .call_on(
                                "Fetch.fulfillRequest",
                                json!({
                                    "requestId": rid,
                                    "responseCode": status,
                                    "body": base64_encode(body.as_bytes()),
                                    "responseHeaders": [
                                        { "name": "Content-Type", "value": content_type },
                                        { "name": "Access-Control-Allow-Origin", "value": "*" },
                                    ],
                                }),
                                sid,
                            )
                            .await;
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

/// 求值值的类型化短预览（报错与 `print` 调试）：标类型加截断内容，字符串
/// 带引号、容器打 JSON 截断；杜绝「JSON 文本看着像对象」的误导（#21）。
fn preview(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => format!("布尔 {b}"),
        Value::Number(n) => format!("数字 {n}"),
        Value::String(s) => format!("字符串\"{}\"", trunc_preview(s)),
        Value::Array(a) => {
            let s = serde_json::to_string(a).unwrap_or_default();
            format!("数组 {}", trunc_preview(&s))
        }
        Value::Object(o) => {
            let s = serde_json::to_string(o).unwrap_or_default();
            format!("对象 {}", trunc_preview(&s))
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
    match v {
        Value::Null => String::new(),
        Value::Array(a) if a.is_empty() => String::new(),
        Value::Object(o) if o.is_empty() => String::new(),
        other => other.to_string(),
    }
}

/// 标准字母表的 base64 解码，容忍空白，不引 crate。
pub(crate) fn base64_decode(s: &str) -> Result<Vec<u8>> {
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
