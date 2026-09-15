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
use tokio::sync::Mutex;

/// 方言宿主：一个 CDP [`Session`] + 一份跨片段持久的变量表。
pub struct JsHost {
    session: Arc<Session>,
    vars: Mutex<HashMap<String, Value>>,
    /// 元素引用表（D35-lite）：最近一次 `snapshot()` 的短 ref -> backendNodeId，
    /// 连同页窗口的代标记（主动代际失效）。整表随 snapshot 替换。
    refs: Mutex<Option<RefTable>>,
    /// 进行中的录制（至多一场；方言面 recordStart/recordStop 管理）。
    record: Mutex<Option<crate::record::Recorder>>,
}

/// 引用表：短 ref -> backendNodeId，加 snapshot 时刻的页窗口代标记。
/// 代标记不匹配（文档被导航重开）即整表作废；SPA 同文档跳转（pushState）
/// 不重开窗口、代不变，ref 继续有效——比 URL 对比零假阳性。
#[derive(Clone)]
struct RefTable {
    generation: i64,
    map: HashMap<String, i64>,
}

impl JsHost {
    /// 绑定一条会话建宿主。变量表为空，随 daemon 生命期累积。
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let host = browse_core::JsHost::new(cdp::Session::new());
    /// ```
    pub fn new(session: Arc<Session>) -> Arc<Self> {
        spawn_dialog_watcher(session.clone());
        Arc::new(Self {
            session,
            vars: Mutex::new(HashMap::new()),
            refs: Mutex::new(None),
            record: Mutex::new(None),
        })
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

    /// 共享的会话（health/status 面用）。
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
                bail!(
                    "不能调用 {}.{}；下一步：可调用的是宿主全局函数或 session.<Domain>.<method>(params)",
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
                // 并整表替换引用表——只有最近一次 snapshot 的 ref 有效
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
                "未知函数 {other}；下一步：可用全局 listPageTargets()/resolveWsUrl()/detectBrowsers()/cdpMethods(domain?)/snapshot()/screenshot(path?, full?)/pdf(path?)/newTab(url?)/switchTab(id)/currentTab()/closeTab(id?)/clickAt(x,y)/fillInput(sel,text)/clickRef(ref)/fillRef(ref,text)/selectOption(ref,value)/pressKey(key)/dialogStatus()/dialogAccept(text?)/dialogDismiss()/waitLoad(ms?)/waitIdle(ms?)/recordStart(opts?)/recordStop()/print(x)；CDP 走 session.<Domain>.<method>(params)"
            ),
        }
    }

    /// 查短 ref 对应的 backendNodeId（只认最近一次 snapshot 的表），
    /// 并做主动代际校验：snapshot 时在页窗口盖过 `__browse_ref_gen` 代标记，
    /// 引用前核对——不匹配即文档已被导航重开，整表作废，给重取 CTA。
    /// 标记取不到（evaluate 失败）不拦，退给被动失效（resolveNode/零尺寸）。
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

/// 对话框 watcher（吸收 agent-browser 语义，游离常驻任务）：给新活动
/// session 补 `Page.enable`（对话框事件需要域开启才流动），`alert`/
/// `beforeunload` 自动接受（`BROWSE_NO_AUTO_DIALOG=1` 关掉），永不阻塞
/// agent；`confirm`/`prompt` 留给显式 `dialogAccept/dialogDismiss`。
/// 状态由 cdp `route()` 截获维护（[`cdp::Session::pending_dialog`]）。
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

fn preview(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// 把方言求值结果渲染成 CLI stdout 文本：标量裸打、空容器不打、其余打 JSON。
///
/// # Examples
///
/// ```
/// use serde_json::json;
/// assert_eq!(browse_core::render_result(&json!("hi")), "hi");
/// assert_eq!(browse_core::render_result(&json!({})), "");
/// assert_eq!(browse_core::render_result(&json!({"a": 1})), r#"{"a":1}"#);
/// ```
pub fn render_result(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Array(a) if a.is_empty() => String::new(),
        Value::Object(o) if o.is_empty() => String::new(),
        other => other.to_string(),
    }
}

/// 极简 base64 解码（标准字母表，容忍空白；不引 crate）。
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
