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
use cdp::{ConnectOptions, PageTarget, Session};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// 方言宿主：一个 CDP [`Session`] + 一份跨片段持久的变量表。
pub struct JsHost {
    session: Arc<Session>,
    vars: Mutex<HashMap<String, Value>>,
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
        Arc::new(Self {
            session,
            vars: Mutex::new(HashMap::new()),
        })
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
                        .ok_or_else(|| anyhow!("未定义变量 {name}"))
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
                bail!("不能调用 {}.{}", preview(&o), prop)
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
            "print" => {
                eprintln!("{}", preview(&argv.first().cloned().unwrap_or(Value::Null)));
                Ok(Value::Null)
            }
            other => bail!("未知函数 {other}"),
        }
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
                    .ok_or_else(|| anyhow!("session.use(targetId) 需要字符串"))?;
                let sid = self.session.use_target(id).await?;
                Ok(json!(sid))
            }
            "waitFor" | "wait_for" => {
                let ev = argv
                    .first()
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow!("waitFor(method, pred?, timeoutMs?)"))?;
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
                    .ok_or_else(|| anyhow!("session.call(method, params?)"))?;
                let params = argv.get(1).cloned().unwrap_or(json!({}));
                self.session.call(method, params).await
            }
            "isConnected" => Ok(json!(self.session.is_connected())),
            "getActiveSession" => Ok(json!(self.session.get_active_session().await)),
            other => bail!("未知 session.{other}"),
        }
    }
}

fn tab_json(t: PageTarget) -> Value {
    json!({
        "targetId": t.target_id,
        "title": t.title,
        "url": t.url,
        "type": t.type_,
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
