//! 语义层近期面：tab 族、交互三件、等待判官。
//!
//! 对齐 harness(py) helpers 的高频操作面，坑表教训直接落地：
//! - `new_tab` 先 about:blank 再 goto（带 url 与 attach 竞速 -> readyState 假完成）
//! - `fill_input` 清空不发 Ctrl+A（char 事件会输入字面 a），用
//!   `commands:['SelectAll']`；填完回读验证，失败如实报
//! - 不自动 `Target.activateTarget`（人机共存：不抢用户前台）；从未激活的
//!   后台 tab 收 Input 若挂起，错误 CTA 提示手动激活
//!
//! 全部函数作用于当前活动 tab（`session.use` 路由）。

use anyhow::{Result, anyhow, bail};
use cdp::Session;
use serde_json::{Value, json};
use std::time::Duration;

/// 新开 tab 并设为活动路由。给了 `url` 则先建 about:blank 附着后再导航
/// （防竞速假完成），返回 `{targetId,title,url}`。
///
/// # Errors
///
/// 未连接、createTarget/navigate 失败（域策略拦截同 `Page.navigate`）。
pub async fn new_tab(s: &Session, url: Option<&str>) -> Result<Value> {
    let id = s.create_target("about:blank").await?;
    let sid = s.use_target(&id).await?;
    // Page 域同步补开（#19）：导航后立即 waitFor 事件不再竞开域时序
    crate::js_host::ensure_page_enabled(s, &sid).await;
    if let Some(u) = url {
        s.call("Page.navigate", json!({ "url": u })).await?;
        // target 列表的 title/url 在 load 前是滞后的 about:blank，等完再取
        // 简表；预算与 goto 缺省对齐 15 秒（#57 G3：原 8 秒是唯一没口径
        // 的裸魔法数）
        let _ = wait_load(s, 15_000).await;
    }
    tab_brief(s, &id).await
}

/// 切换活动路由到既有 tab（不改 Chrome 可见前景），返回该 tab 简表。
///
/// # Errors
///
/// 未连接或 attach 失败（targetId 不存在）。
pub async fn switch_tab(s: &Session, target_id: &str) -> Result<Value> {
    let sid = s.use_target(target_id).await?;
    crate::js_host::ensure_page_enabled(s, &sid).await;
    tab_brief(s, target_id).await
}

/// 当前活动 tab 简表 `{targetId,title,url}`；无活动 tab 返回 `null`。
///
/// # Errors
///
/// 未连接或 `Target.getTargets` 失败。
pub async fn current_tab(s: &Session) -> Result<Value> {
    let want = s.active_target().await;
    let tabs = s.list_page_targets().await?;
    Ok(tabs
        .iter()
        .find(|t| Some(&t.target_id) == want.as_ref())
        .map(|t| json!({ "targetId": t.target_id, "title": t.title, "url": t.url, "own": t.own }))
        .unwrap_or(Value::Null))
}

/// 关 tab（缺省关当前活动 tab）；守卫层只放行本会话自建 tab，用户 tab 一律拒绝。
///
/// # Errors
///
/// 目标不是自建 tab（守卫拦截），或 closeTarget 失败。
pub async fn close_tab(s: &Session, target_id: Option<&str>) -> Result<Value> {
    let id = match target_id {
        Some(id) => id.to_string(),
        None => s
            .active_target()
            .await
            .ok_or_else(|| anyhow!("closeTab 无活动 tab；下一步：closeTab(\"<targetId>\")，先 const tabs = await listPageTargets()"))?,
    };
    s.call("Target.closeTarget", json!({ "targetId": id }))
        .await?;
    Ok(json!(true))
}

/// 一步导航（#19）：`Page.navigate` 加 waitLoad 一体收尾，可选再等网络静默，
/// 返回提交后世界的 url/title/elapsedMs。用于「去这个页且等它可用」，替代
/// 手写 navigate 加 waitLoad 两步；也替代 navigate 加 waitFor(loadEventFired)
/// 组合——后者事件已发再注册即假超时（#19 竞速），本函数走 readyState 轮询
/// 无此窗。
///
/// `timeout_ms` 是导航加载合计预算；`idle_ms` 为 Some 时再等网络静默。
/// url/title 取自目标元数据（Target.getTargets 面），瞬时提交窗内可能
/// 滞后；权威读取用 `location.href` / `document.title`（评审 G7）。
///
/// # Errors
///
/// navigate 回执带 errorText；预算内 readyState 未到 complete；idle 档
/// 未静默。
pub async fn goto(s: &Session, url: &str, timeout_ms: u64, idle_ms: Option<u64>) -> Result<Value> {
    let t0 = std::time::Instant::now();
    let nav = s.call("Page.navigate", json!({ "url": url })).await?;
    if let Some(et) = nav.pointer("/errorText").and_then(Value::as_str) {
        bail!("goto({url}) 导航失败：{et}；下一步：核对 url，或 session.Page.navigate 看完整回执");
    }
    wait_load(s, timeout_ms).await?;
    if let Some(ms) = idle_ms {
        wait_idle(s, ms).await?;
    }
    let tab = current_tab(s).await?;
    Ok(json!({
        "url": tab.get("url"),
        "title": tab.get("title"),
        "elapsedMs": t0.elapsed().as_millis() as u64,
    }))
}

/// 历史回退（#39）：`Page.getNavigationHistory` 取 currentIndex，回退 delta
/// 步（缺省 1，越界钳到最早条目）后 `navigateToHistoryEntry`，再等加载
/// 收尾。返回实跳步数与落点 url/title。
///
/// # Errors
///
/// 历史为空；历史查询或导航失败。
pub async fn go_back(s: &Session, delta: u64) -> Result<Value> {
    history_jump(s, -(delta.max(1) as i64)).await
}

/// 历史前进（#39）：同 [`go_back`] 方向相反，钳到最新条目。
///
/// # Errors
///
/// 同 [`go_back`]。
pub async fn go_forward(s: &Session, delta: u64) -> Result<Value> {
    history_jump(s, delta.max(1) as i64).await
}

async fn history_jump(s: &Session, delta: i64) -> Result<Value> {
    let h = s.call("Page.getNavigationHistory", json!({})).await?;
    let entries = h
        .pointer("/entries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if entries.is_empty() {
        bail!("历史为空，无条目可跳；下一步：先 goto(url) 建立历史");
    }
    let len = entries.len() as i64;
    let idx = h
        .pointer("/currentIndex")
        .and_then(Value::as_i64)
        .filter(|i| *i >= 0 && *i < len)
        .ok_or_else(|| anyhow!(
            "历史 currentIndex 缺失或越界；下一步：裸调 session.Page.getNavigationHistory 看回执形态"
        ))?;
    let to = (idx + delta).clamp(0, len - 1);
    let entry = &entries[to as usize];
    let fallback_url = entry
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let entry_id = entry.get("id").and_then(Value::as_i64).ok_or_else(|| anyhow!(
        "历史条目缺 id 字段（entryId 是不透明整数，不可用索引顶替）；下一步：裸调 session.Page.getNavigationHistory 看回执形态"
    ))?;
    let since = s.last_seq().await;
    s.call(
        "Page.navigateToHistoryEntry",
        json!({ "entryId": entry_id }),
    )
    .await?;
    // 历史跳无提交屏障背书（屏障只认 Page.navigate），走有界提交等待
    // （评审二轮 F4）：同文档 fragment 跳无导航事件，grace 后即返回
    wait_settled(s, since, 2_000, 15_000).await?;
    let tab = current_tab(s).await?;
    Ok(json!({
        "steps": to - idx,
        "url": tab.get("url").cloned().unwrap_or(json!(fallback_url)),
        "title": tab.get("title"),
    }))
}

/// 刷新当前页（#39）：`Page.reload`（可带 ignoreCache）后等加载收尾。
///
/// # Errors
///
/// reload 调用失败或加载预算内 readyState 未到 complete。
pub async fn reload(s: &Session, ignore_cache: bool) -> Result<Value> {
    let t0 = std::time::Instant::now();
    let since = s.last_seq().await;
    s.call("Page.reload", json!({ "ignoreCache": ignore_cache }))
        .await?;
    // reload 无提交屏障背书，走有界提交等待（评审二轮 F4）
    wait_settled(s, since, 2_000, 20_000).await?;
    let tab = current_tab(s).await?;
    Ok(json!({
        "ignoredCache": ignore_cache,
        "url": tab.get("url"),
        "title": tab.get("title"),
        "elapsedMs": t0.elapsed().as_millis() as u64,
    }))
}

/// 等导航落定（评审二轮 F4）：reload、历史跳、点击后导航的通用收尾。
/// 先在 grace 窗内探「提交已在途」（自 since 起 frameStartedLoading 或
/// frameNavigated 有新事件），在途则走 [`wait_load`] 等收尾并标
/// settled=nav；grace 窗内无导航迹象即标 settled=no-nav 返回（同文档
/// fragment 与纯 JS 按钮不误等）。通用 waitLoad() 不经本函数（保持已
/// 加载页立即返回）；提交屏障只护 Page.navigate，本函数补其余导航面。
///
/// # Errors
///
/// 在途路径下 wait_load 预算内未到 complete。
pub async fn wait_settled(s: &Session, since: u64, grace_ms: u64, budget_ms: u64) -> Result<Value> {
    let t0 = std::time::Instant::now();
    let grace_deadline = tokio::time::Instant::now() + Duration::from_millis(grace_ms);
    loop {
        let started = s
            .peek_events_since("Page.frameStartedLoading", since, 10)
            .await
            .len()
            + s.peek_events_since("Page.frameNavigated", since, 10)
                .await
                .len();
        if started > 0 {
            let mut r = wait_load(s, budget_ms).await?;
            if let Some(o) = r.as_object_mut() {
                o.insert("settled".to_string(), json!("nav"));
            }
            return Ok(r);
        }
        if tokio::time::Instant::now() >= grace_deadline {
            // 无导航迹象：原文档照旧，readyState 即真态，不构成早返误判
            let r = s
                .call(
                    "Runtime.evaluate",
                    json!({ "expression": "document.readyState", "returnByValue": true }),
                )
                .await;
            let rs = r
                .ok()
                .and_then(|v| {
                    v.pointer("/result/value")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_default();
            // 两态同形（评审三轮新 G）：no-nav 也带 elapsedMs
            return Ok(json!({
                "readyState": rs,
                "settled": "no-nav",
                "elapsedMs": t0.elapsed().as_millis() as u64,
            }));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// a11y 媒质仿真族（#40）：`Emulation.setEmulatedMedia` 的 features 面。
/// `opts` 任给其一：`colorScheme`（dark/light）、`reducedMotion`
/// （reduce/no-preference）、`forcedColors`（active/none）、`prefersContrast`
/// （more/less/no-preference）、`media`（screen/print）。页内以
/// `matchMedia("(prefers-color-scheme: dark)")` 等感知；还原走
/// [`emulate_media_clear`]。
///
/// # Errors
///
/// 未连接；opts 一项都没有；CDP 拒绝（枚举值写错原样透传，守卫 CTA 指路）。
pub async fn emulate_media(s: &Session, opts: &Value) -> Result<Value> {
    let mut params = json!({});
    let mut features: Vec<Value> = Vec::new();
    let mut push = |name: &str, v: Option<&str>| {
        if let Some(v) = v {
            features.push(json!({ "name": name, "value": v }));
        }
    };
    // 五参白名单（#40 评审 G2/G3）：非法枚举当场 bail 列合法值，不静默
    // true；未知键与值类型错分叉归因
    const KNOWN: [&str; 5] = [
        "colorScheme",
        "reducedMotion",
        "forcedColors",
        "prefersContrast",
        "media",
    ];
    for k in opts
        .as_object()
        .map(|o| o.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default()
    {
        if !KNOWN.contains(&k.as_str()) {
            bail!(
                "emulateMedia 不认识的键 {k}（可认五键：colorScheme/reducedMotion/forcedColors/prefersContrast/media）"
            );
        }
        if !opts.get(&k).is_some_and(Value::is_string) {
            bail!("emulateMedia 的 {k} 值要是字符串");
        }
    }
    let check = |k: &str, who: &str, allowed: &[&str]| -> Result<()> {
        if let Some(v) = opts.get(k).and_then(Value::as_str)
            && !allowed.contains(&v)
        {
            bail!(
                "emulateMedia 的 {who} 非法值 {v}（合法：{}）",
                allowed.join("/")
            );
        }
        Ok(())
    };
    check(
        "colorScheme",
        "colorScheme",
        &["dark", "light", "no-preference"],
    )?;
    check(
        "reducedMotion",
        "reducedMotion",
        &["reduce", "no-preference"],
    )?;
    check("forcedColors", "forcedColors", &["active", "none"])?;
    check(
        "prefersContrast",
        "prefersContrast",
        &["more", "less", "no-preference"],
    )?;
    check("media", "media", &["screen", "print"])?;
    push(
        "prefers-color-scheme",
        opts.get("colorScheme").and_then(Value::as_str),
    );
    push(
        "prefers-reduced-motion",
        opts.get("reducedMotion").and_then(Value::as_str),
    );
    push(
        "forced-colors",
        opts.get("forcedColors").and_then(Value::as_str),
    );
    push(
        "prefers-contrast",
        opts.get("prefersContrast").and_then(Value::as_str),
    );
    if let Some(m) = opts.get("media").and_then(Value::as_str) {
        params["media"] = json!(m);
    }
    if !features.is_empty() {
        params["features"] = json!(features);
    }
    if params.as_object().is_some_and(serde_json::Map::is_empty) {
        bail!(
            "emulateMedia 至少给一项；下一步：emulateMedia({{colorScheme: \"dark\"}}) 或 {{media: \"print\"}}（colorScheme/reducedMotion/forcedColors/prefersContrast/media 五选一以上）"
        );
    }
    s.call("Emulation.setEmulatedMedia", params).await?;
    Ok(json!(true))
}

/// 还原媒质仿真（#40）：`Emulation.setEmulatedMedia` 空参，五特征与媒质
/// 全部回 stock。
///
/// # Errors
///
/// 未连接或 CDP 失败。
pub async fn emulate_media_clear(s: &Session) -> Result<Value> {
    s.call("Emulation.setEmulatedMedia", json!({})).await?;
    Ok(json!(true))
}

/// 列 cookie（#42）：`Network.getCookies`。无参是当前页 URL 作用域
/// （CDP 按活动 target 的 URL 解析，非全 jar，实弹口径）；给了 domain
/// 则按该域 http/https 两 URL 显式过滤。回 cookie 简表数组（原生字段）。
///
/// # Errors
///
/// 未连接或 CDP 失败。
pub async fn cookies(s: &Session, domain: Option<&str>) -> Result<Value> {
    let params = match domain {
        Some(d) => json!({ "urls": [format!("https://{d}/"), format!("http://{d}/")] }),
        None => json!({}),
    };
    let r = s.call("Network.getCookies", params).await?;
    Ok(r.get("cookies").cloned().unwrap_or(json!([])))
}

/// 写单条 cookie（#42）：`Network.setCookie`。`opts` 可带 domain（缺省用
/// 当前页 URL 的域）、path、expires（Unix 秒）、httpOnly、secure、
/// sameSite。回 CDP 的 success 布尔。
///
/// # Errors
///
/// 未连接或 CDP 拒绝（如缺 domain 又无当前页）。
pub async fn cookie_set(s: &Session, name: &str, value: &str, opts: &Value) -> Result<Value> {
    let mut params = json!({ "name": name, "value": value });
    if let Some(d) = opts.get("domain").and_then(Value::as_str) {
        params["domain"] = json!(d);
    } else {
        // 缺 domain 走当前页 URL（CDP 要求 url 或 domain 二选一）
        let url = s
            .call(
                "Runtime.evaluate",
                json!({
                    "expression": "location.href", "returnByValue": true
                }),
            )
            .await
            .ok()
            .and_then(|v| {
                v.pointer("/result/value")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            });
        match url {
            // scheme 预检（#42 评审 G3）：data:/about: 等页直接给行动指令，
            // 不让 CDP 的 scheme 报错裸透传
            Some(u) if u.starts_with("http://") || u.starts_with("https://") => {
                params["url"] = json!(u);
            }
            Some(_) => {
                bail!(
                    "cookieSet 当前页不是 http/https 源；下一步：cookieSet(name, value, {{domain: \"example.com\"}}) 显式给域"
                );
            }
            None => {
                bail!(
                    "cookieSet 缺 domain 且无当前页 URL；下一步：cookieSet(name, value, {{domain: \"example.com\"}})"
                )
            }
        }
    }
    for k in ["path", "expires", "httpOnly", "secure", "sameSite"] {
        if let Some(v) = opts.get(k) {
            params[k] = v.clone();
        }
    }
    let r = s.call("Network.setCookie", params).await?;
    Ok(r.get("success").cloned().unwrap_or(json!(true)))
}

/// 删单条 cookie（#42）：`Network.deleteCookies`（CDP 无单数形，按 name 加域删全部匹配），缺省
/// 当前页 URL 域）。
///
/// # Errors
///
/// 未连接或 CDP 失败。
pub async fn cookie_delete(s: &Session, name: &str, domain: Option<&str>) -> Result<Value> {
    let mut params = json!({ "name": name });
    match domain {
        Some(d) => params["domain"] = json!(d),
        None => {
            let url = s
                .call(
                    "Runtime.evaluate",
                    json!({
                        "expression": "location.href", "returnByValue": true
                    }),
                )
                .await
                .ok()
                .and_then(|v| {
                    v.pointer("/result/value")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                });
            match url {
                Some(u) => params["url"] = json!(u),
                None => bail!(
                    "cookieDelete 缺 domain 且无当前页 URL；下一步：cookieDelete(name, {{domain: \"example.com\"}})"
                ),
            }
        }
    }
    s.call("Network.deleteCookies", params).await?;
    Ok(json!(true))
}

/// 清空浏览器全部 cookie（#42）：`Network.clearBrowserCookies`（对照
/// playwright cookie-clear，作用面是整个浏览器不只是当前域）。
///
/// # Errors
///
/// 未连接或 CDP 失败。
pub async fn cookies_clear(s: &Session) -> Result<Value> {
    s.call("Network.clearBrowserCookies", json!({})).await?;
    Ok(json!(true))
}

/// Web Storage 逐键 CRUD 的统一执行面（#42）：`which` 是 localStorage 或
/// sessionStorage，op 是 get/set/remove/clear，键值经 JSON 序列化内嵌
/// 防注入。回页内表达式的原值（get 的 null 表示键不存在）。
///
/// # Errors
///
/// 未连接或页内求值失败。
pub async fn storage_op_pub(
    s: &Session,
    which: &str,
    op: &str,
    key: Option<&str>,
    value: Option<&str>,
) -> Result<Value> {
    let expr = match (op, key, value) {
        ("get", Some(k), _) => format!("{which}.getItem({})", serde_json::to_string(k)?),
        ("set", Some(k), Some(v)) => format!(
            "(() => {{ {which}.setItem({}, {}); return {which}.getItem({}); }})()",
            serde_json::to_string(k)?,
            serde_json::to_string(v)?,
            serde_json::to_string(k)?
        ),
        ("remove", Some(k), _) => format!("{which}.removeItem({})", serde_json::to_string(k)?),
        ("clear", _, _) => format!("{which}.clear()"),
        _ => bail!("storage 内部形态错：{op}/{key:?}"),
    };
    let r = s
        .call(
            "Runtime.evaluate",
            json!({ "expression": expr, "returnByValue": true }),
        )
        .await?;
    Ok(r.pointer("/result/value").cloned().unwrap_or(Value::Null))
}

/// 勾选/取消复选框（#39）：读元素 checked 实态，与目标态不一致才点击
/// （checkRef 后必为 true，重复调用幂等）。radio 只能置 true：已选中的
/// radio 再 uncheck 无意义，原样返回不点击。
///
/// # Errors
///
/// 元素不是 checkbox/radio；元素 disabled；状态读取或点击失败。
pub async fn set_checked(s: &Session, backend_node_id: i64, checked: bool) -> Result<Value> {
    let object_id = resolve_node_object(s, backend_node_id).await?;
    let r = s
        .call(
            "Runtime.callFunctionOn",
            json!({
                "objectId": object_id,
                "functionDeclaration": r#"function(){ return {
                    tag: this.tagName,
                    type: (this.type || ''),
                    checked: !!this.checked,
                    enabled: !this.disabled
                }; }"#,
                "returnByValue": true
            }),
        )
        .await?;
    let st = r.pointer("/result/value").cloned().unwrap_or(Value::Null);
    let tag = st
        .get("tag")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_uppercase();
    let ty = st.get("type").and_then(Value::as_str).unwrap_or("");
    if tag != "INPUT" || (ty != "checkbox" && ty != "radio") {
        bail!(
            "check/uncheck 只适用 checkbox 与 radio（当前 {tag} type={ty}）；下一步：普通元素用 clickRef"
        );
    }
    if !st.get("enabled").and_then(Value::as_bool).unwrap_or(false) {
        bail!("元素 disabled；下一步：先启用再勾选");
    }
    let cur = st.get("checked").and_then(Value::as_bool).unwrap_or(false);
    if cur == checked || (ty == "radio" && cur) {
        return Ok(json!({ "checked": cur, "clicked": false }));
    }
    click_ref(s, backend_node_id).await?;
    Ok(json!({ "checked": checked, "clicked": true }))
}

/// 用 `Input.dispatchMouseEvent` pressed+released 在视口坐标 (x,y) 派发
/// trusted 的真点击。
///
/// 坐标命中的是当前可见物；不自动激活 tab（人机共存）。
///
/// # Errors
///
/// 未连接或派发失败。从未激活的后台 tab 可能挂起（30s 超时），届时按错误
/// CTA 先 `session.Target.activateTarget({targetId})`。
pub async fn click_at(s: &Session, x: i64, y: i64) -> Result<Value> {
    click_at_opts(s, x, y, "left", 1).await
}

/// clickAt 的参数化半边（#35）：button（left/right/middle/back/forward）
/// 与 clickCount（2 即双击语义）。
///
/// # Errors
///
/// 同 [`click_at`]。
pub async fn click_at_opts(
    s: &Session,
    x: i64,
    y: i64,
    button: &str,
    click_count: i64,
) -> Result<Value> {
    validate_button(button)?;
    dispatch_input_seq(
        s,
        vec![
            ("Input.dispatchMouseEvent", json!({ "type": "mousePressed", "x": x, "y": y, "button": button, "clickCount": click_count })),
            ("Input.dispatchMouseEvent", json!({ "type": "mouseReleased", "x": x, "y": y, "button": button, "clickCount": click_count })),
        ],
    )
    .await?;
    Ok(json!(true))
}

/// 最近一次 mouseMove/hover 的坐标（#35 评审 F1）：mouseDown/mouseUp/
/// mouseWheel 缺省落点。进程级单份（daemon 单引擎单活动页，口径够用）。
static LAST_MOUSE: std::sync::Mutex<Option<(i64, i64)>> = std::sync::Mutex::new(None);

/// 量元素视口矩形（#41）：滚动可见后取 rect，回 (x, y, w, h)；不可见
/// 返回 None（调用方决定报错或全页截）。pub 供 js_host 的元素级截图用。
///
/// # Errors
///
/// 页内求值失败。
pub async fn element_rect(s: &Session, object_id: &str) -> Result<Option<(f64, f64, f64, f64)>> {
    let r = s
        .call(
            "Runtime.callFunctionOn",
            json!({
                "objectId": object_id,
                "functionDeclaration": r#"function(){
                    this.scrollIntoView({block: 'center'});
                    const r = this.getBoundingClientRect();
                    if (!this.isConnected || (!r.width && !r.height)) return null;
                    return JSON.stringify({x: r.x, y: r.y, w: r.width, h: r.height});
                }"#,
                "returnByValue": true
            }),
        )
        .await?;
    let v = r
        .pointer("/result/value")
        .and_then(Value::as_str)
        .and_then(|t| serde_json::from_str::<Value>(t).ok());
    Ok(v.map(|v| {
        (
            v.get("x").and_then(Value::as_f64).unwrap_or(0.0),
            v.get("y").and_then(Value::as_f64).unwrap_or(0.0),
            v.get("w").and_then(Value::as_f64).unwrap_or(0.0),
            v.get("h").and_then(Value::as_f64).unwrap_or(0.0),
        )
    }))
}

/// 持久高亮覆盖层（#41）：给元素画 2px 橙框加可选编号徽标（label），不
/// 挡点击（pointer-events: none）；幂等（同元素重复高亮刷新框位）。清场
/// 走上层 highlightClear（按 data-browse-hl 属性移除）。
///
/// # Errors
///
/// 元素不可见（量不到 rect）或求值失败。
pub async fn highlight(s: &Session, backend_node_id: i64, label: Option<&str>) -> Result<Value> {
    let object_id = resolve_node_object(s, backend_node_id).await?;
    let r = s
        .call(
            "Runtime.callFunctionOn",
            json!({
                "objectId": object_id,
                "functionDeclaration": r#"function(label){
                    this.scrollIntoView({block: 'center'});
                    const r = this.getBoundingClientRect();
                    if (!this.isConnected || (!r.width && !r.height)) return false;
                    const key = 'browse-hl-' + (this.dataset.browseHlKey || Math.random().toString(36).slice(2));
                    this.dataset.browseHlKey = key;
                    let box = document.getElementById(key);
                    if (!box) {
                        box = document.createElement('div');
                        box.id = key; box.dataset.browseHl = '1';
                        // 页面坐标绝对定位（评审 F1）：滚动后框跟随目标；
                        // fixed 加调用时刻写死坐标会漂移误导人眼
                        box.style.cssText = 'position:absolute;pointer-events:none;z-index:2147483647;border:2px solid #ff8c00;background:rgba(255,140,0,.12)';
                        document.documentElement.appendChild(box);
                    }
                    const px = r.x + window.scrollX, py = r.y + window.scrollY;
                    box.style.left = px + 'px'; box.style.top = py + 'px';
                    box.style.width = r.width + 'px'; box.style.height = r.height + 'px';
                    if (label) {
                        let tag = document.getElementById(key + '-tag') || (() => {
                            const t = document.createElement('div');
                            t.id = key + '-tag'; t.dataset.browseHl = '1';
                            t.style.cssText = 'position:absolute;pointer-events:none;z-index:2147483647;background:#ff8c00;color:#fff;font:bold 12px monospace;padding:1px 4px;border-radius:3px';
                            document.documentElement.appendChild(t); return t;
                        })();
                        tag.textContent = label;
                        tag.style.left = px + 'px'; tag.style.top = (py - 16) + 'px';
                    }
                    return true;
                }"#,
                "arguments": [json!({ "value": label.unwrap_or("") })],
                "returnByValue": true
            }),
        )
        .await?;
    let ok = r.pointer("/result/value") == Some(&json!(true));
    if !ok {
        bail!("highlight 量不到元素（不可见或已失效）；下一步：重新 snapshot() 取新 ref");
    }
    Ok(json!(true))
}

/// 鼠标按键白名单（#35 评审 G1）：非法值当场报错列合法值，不靠 CDP 的
/// Invalid mouse button（Playwright 别名 primary/secondary 会静默不派发）。
fn validate_button(button: &str) -> Result<()> {
    const OK: [&str; 5] = ["left", "right", "middle", "back", "forward"];
    if !OK.contains(&button) {
        bail!("button 非法值 {button}（合法：left/right/middle/back/forward）");
    }
    Ok(())
}

/// 鼠标原语族（#35）：move、按下/释放分离、滚轮、按钮与次数参数化。
/// `button` 取 left/right/middle/back/forward；clickCount 给 2 即双击
/// 语义（dblclick）。全部走 `Input.dispatchMouseEvent` trusted 派发。
///
/// # Errors
///
/// 未连接或派发失败（后台 tab 挂起口径同 clickAt）。
pub async fn mouse_move(s: &Session, x: i64, y: i64) -> Result<Value> {
    s.call(
        "Input.dispatchMouseEvent",
        json!({ "type": "mouseMoved", "x": x, "y": y }),
    )
    .await?;
    if let Ok(mut m) = LAST_MOUSE.lock() {
        *m = Some((x, y));
    }
    // #43 录制光标跟随：元素在位才动（页面坐标口径同高亮；不在录时该
    // evaluate 是廉价 no-op）。坐标内嵌走数字字面量（i64 无注入面）
    let _ = s
        .call(
            "Runtime.evaluate",
            json!({
                "expression": format!(
                    "(() => {{ const c = document.getElementById('browse-rec-cursor'); if (c) {{ c.style.left = (window.scrollX + {x}) + 'px'; c.style.top = (window.scrollY + {y}) + 'px'; }} return true }})()"
                ),
                "returnByValue": true
            }),
        )
        .await;
    Ok(json!(true))
}

/// 按下不释放（#35）：拖拽与长按语义的半边。
///
/// # Errors
///
/// 同 [`mouse_move`]。
pub async fn mouse_down(
    s: &Session,
    x: Option<i64>,
    y: Option<i64>,
    button: &str,
) -> Result<Value> {
    validate_button(button)?;
    let (x, y) = resolve_mouse_pos(x, y);
    // 先 move 到落点（Chrome 输入状态机：按下落在当前指针位置）
    s.call(
        "Input.dispatchMouseEvent",
        json!({ "type": "mouseMoved", "x": x, "y": y }),
    )
    .await?;
    s.call(
        "Input.dispatchMouseEvent",
        json!({ "type": "mousePressed", "x": x, "y": y, "button": button, "clickCount": 1 }),
    )
    .await?;
    Ok(json!(true))
}

/// 释放（#35）：与 [`mouse_down`] 配对。
///
/// # Errors
///
/// 同 [`mouse_move`]。
pub async fn mouse_up(s: &Session, x: Option<i64>, y: Option<i64>, button: &str) -> Result<Value> {
    validate_button(button)?;
    let (x, y) = resolve_mouse_pos(x, y);
    s.call(
        "Input.dispatchMouseEvent",
        json!({ "type": "mouseReleased", "x": x, "y": y, "button": button, "clickCount": 1 }),
    )
    .await?;
    Ok(json!(true))
}

/// 鼠标落点解析（#35 评审 F1）：显式坐标优先，缺省沿用最近 mouseMove，
/// 再缺省 (0,0)。
fn resolve_mouse_pos(x: Option<i64>, y: Option<i64>) -> (i64, i64) {
    let last = LAST_MOUSE.lock().ok().and_then(|m| *m);
    (
        x.or(last.map(|l| l.0)).unwrap_or(0),
        y.or(last.map(|l| l.1)).unwrap_or(0),
    )
}

/// 滚轮（#35）：deltaX/deltaY 是像素量（向下滚正 deltaY）；触发 wheel
/// 事件路径（SPA 懒加载监听 wheel 时 JS scrollBy 不可替代）。
///
/// # Errors
///
/// 同 [`mouse_move`]。
pub async fn mouse_wheel(s: &Session, dx: i64, dy: i64) -> Result<Value> {
    // 走 synthesizeScrollGesture（#35 实测定谳）：dispatchMouseEvent 的
    // mouseWheel 在导航后有首发吞没（首个被渲染器当监听注册握手消耗，
    // 第二发起才触发，w1=0/w2=1/w3=2 三连实测）；手势合成连续 wheel 流
    // 首次即触发。要精确单 wheel 事件就裸调 dispatchMouseEvent（自双发）
    let (px, py) = resolve_mouse_pos(None, None);
    let (px, py) = if (px, py) == (0, 0) {
        (50, 50)
    } else {
        (px, py)
    };
    s.call(
        "Input.synthesizeScrollGesture",
        json!({ "x": px, "y": py, "xDistance": -dx, "yDistance": -dy, "speed": 800 }),
    )
    .await?;
    Ok(json!(true))
}

/// 按 CSS 选择器填输入框：focus -> 全选（commands，不发 Ctrl+A）-> 可选
/// Backspace 清空 -> `Input.insertText` -> 回读严格验证。
///
/// 只支持文本类控件；`<select>` 用 `Runtime.evaluate` 设 value 并派发
/// change 事件（CTA 给写法）。
///
/// # Errors
///
/// 选择器未命中、readOnly、回读不一致（错误附回读值）。
pub async fn fill_input(s: &Session, selector: &str, text: &str) -> Result<Value> {
    let probe = format!(
        "(() => {{ const el = document.querySelector({sel_json}); \
          if (!el) return null; el.focus(); \
          return {{tag: el.tagName, ro: el.readOnly === true}}; }})()",
        sel_json = serde_json::to_string(selector).unwrap_or_default()
    );
    let meta = s
        .call(
            "Runtime.evaluate",
            json!({ "expression": probe, "returnByValue": true }),
        )
        .await?;
    let m = meta
        .pointer("/result/value")
        .cloned()
        .ok_or_else(|| anyhow!("fillInput 选择器未命中：{selector}；下一步：先用 snapshot() 或 Runtime.querySelector 确认元素存在"))?;
    if m.is_null() {
        bail!("fillInput 选择器未命中：{selector}；下一步：先 snapshot() 看页面结构再选");
    }
    if m.get("tag").and_then(Value::as_str) == Some("SELECT") {
        bail!(
            "fillInput 暂不支持 <select>；下一步：先 await snapshot() 拿该下拉框的 ref，再 selectOption(ref, \"值或可见 label\")"
        );
    }
    if m.get("ro").and_then(Value::as_bool) == Some(true) {
        bail!("fillInput 目标是 readOnly：{selector}");
    }
    // 全选（commands 路径，规避 Ctrl+A 的字面 'a' 副作用）-> 清空/输入
    let mut seq: Vec<(&'static str, Value)> = vec![
        (
            "Input.dispatchKeyEvent",
            json!({ "type": "rawKeyDown", "key": "a", "code": "KeyA", "commands": ["SelectAll"] }),
        ),
        (
            "Input.dispatchKeyEvent",
            json!({ "type": "keyUp", "key": "a", "code": "KeyA" }),
        ),
    ];
    if text.is_empty() {
        seq.push((
            "Input.dispatchKeyEvent",
            json!({ "type": "rawKeyDown", "key": "Backspace", "code": "Backspace" }),
        ));
        seq.push((
            "Input.dispatchKeyEvent",
            json!({ "type": "keyUp", "key": "Backspace", "code": "Backspace" }),
        ));
    } else {
        seq.push(("Input.insertText", json!({ "text": text })));
    }
    dispatch_input_seq(s, seq).await?;
    // 回读严格验证
    let read = s
        .call(
            "Runtime.evaluate",
            json!({
                "expression": format!("document.querySelector({sel}).value", sel = serde_json::to_string(selector).unwrap_or_default()),
                "returnByValue": true
            }),
        )
        .await?;
    let got = read
        .pointer("/result/value")
        .and_then(Value::as_str)
        .unwrap_or("");
    if got != text {
        bail!(
            "fillInput 回读不一致：期望 {text:?} 实得 {got:?}（选择器 {selector}）；下一步：检查是否有 JS 覆写或格式化输入"
        );
    }
    Ok(json!(text))
}

/// 用 `Input.dispatchKeyEvent` keyDown(+text)+keyUp 按一个键；Enter 的
/// text 是 `\r`（CDP 契约），可打印单字符带自身为 text。
///
/// # Errors
///
/// 未连接或派发失败。
pub async fn press_key(s: &Session, key: &str) -> Result<Value> {
    // 组合键（#23）：「Control+a」拆修饰位与末位键
    let (mods, key) = if key.contains('+') {
        parse_modifiers(key)?
    } else {
        (0, key.to_string())
    };
    let text = if mods > 0 {
        // 修饰组合是快捷键不是输入：不带 text（带 text 会被当字面输入）
        ""
    } else {
        match key.as_str() {
            "Enter" => "\r",
            k if k.chars().count() == 1 => k,
            _ => "",
        }
    };
    let mut down = json!({ "type": "keyDown", "key": key });
    if mods > 0 {
        down["modifiers"] = json!(mods);
        // 虚拟键码补齐：快捷键处理（全选/复制等）看 vkCode 不看 text
        if let Some((code, vk)) = key_code(&key) {
            down["code"] = json!(code);
            down["windowsVirtualKeyCode"] = json!(vk);
            down["nativeVirtualKeyCode"] = json!(vk);
        }
    }
    if !text.is_empty() {
        down["text"] = json!(text);
    }
    let mut up = json!({ "type": "keyUp", "key": key });
    if mods > 0 {
        up["modifiers"] = json!(mods);
        if let Some((code, vk)) = key_code(&key) {
            up["code"] = json!(code);
            up["windowsVirtualKeyCode"] = json!(vk);
            up["nativeVirtualKeyCode"] = json!(vk);
        }
    }
    dispatch_input_seq(
        s,
        vec![
            ("Input.dispatchKeyEvent", down),
            ("Input.dispatchKeyEvent", up),
        ],
    )
    .await?;
    Ok(json!(true))
}

/// 常用键的 code 与 Windows 虚拟键码（#23）：组合快捷键派发补齐用。
fn key_code(key: &str) -> Option<(&'static str, u64)> {
    Some(match key {
        "a" => ("KeyA", 65),
        "b" => ("KeyB", 66),
        "c" => ("KeyC", 67),
        "v" => ("KeyV", 86),
        "x" => ("KeyX", 88),
        "z" => ("KeyZ", 90),
        "A" => ("KeyA", 65),
        "C" => ("KeyC", 67),
        "V" => ("KeyV", 86),
        "X" => ("KeyX", 88),
        "Z" => ("KeyZ", 90),
        _ => return None,
    })
}

/// 解析「修饰+...+键」组合（#23）：返回 CDP modifiers 位（Alt=1 /
/// Control=2 / Meta=4 / Shift=8）与末位键名。
fn parse_modifiers(key: &str) -> Result<(u64, String)> {
    let parts: Vec<&str> = key.split('+').collect();
    if parts.len() < 2 || parts.last().is_none_or(|k| k.is_empty()) {
        bail!("组合键写法不完整（{key}）；下一步：形如 \"Control+a\"，修饰在前末位键在后");
    }
    let (mods, tail) = parts.split_at(parts.len() - 1);
    let mut bits = 0u64;
    for m in mods {
        // |= 防重复修饰串位（Control+Control+a 不能变成 Meta，评审 G1）
        bits |= match *m {
            "Alt" | "AltGraph" => 1,
            "Control" | "Ctrl" => 2,
            "Meta" | "Command" | "Cmd" => 4,
            "Shift" => 8,
            other => bail!(
                "未知修饰键 {other}；下一步：Alt / Control(或 Ctrl) / Meta(或 Cmd) / Shift，组合如 \"Control+Shift+a\""
            ),
        };
    }
    Ok((bits, tail[0].to_string()))
}

/// 裸按键事件（#23）：keydown / keyup 按住语义（无 text，不发组合成键）。
///
/// `down` 为 true 发 `keyDown`，否则 `keyUp`；组合写法同 [`press_key`]。
///
/// # Errors
///
/// 未连接、修饰键名不合法或派发失败。
pub async fn key_raw(s: &Session, key: &str, down: bool) -> Result<Value> {
    let (mods, key) = if key.contains('+') {
        parse_modifiers(key)?
    } else {
        (0, key.to_string())
    };
    let ty = if down { "keyDown" } else { "keyUp" };
    let mut ev = json!({ "type": ty, "key": key });
    if mods > 0 {
        ev["modifiers"] = json!(mods);
    }
    s.call("Input.dispatchKeyEvent", ev).await?;
    Ok(json!(true))
}

/// 移动鼠标到视口坐标（#23）：触发 `:hover` 与悬停菜单的 mouseMoved。
///
/// # Errors
///
/// 未连接或派发失败。
pub async fn hover_at(s: &Session, x: i64, y: i64) -> Result<Value> {
    s.call(
        "Input.dispatchMouseEvent",
        json!({ "type": "mouseMoved", "x": x, "y": y }),
    )
    .await?;
    Ok(json!(true))
}

/// 量元素视口中心（#23）：scrollIntoView 后取 rect 中心；不可见即报
/// （CTA 同 clickRef 口径）。
async fn node_center(s: &Session, backend_node_id: i64) -> Result<(f64, f64)> {
    let object_id = resolve_node_object(s, backend_node_id).await?;
    let r = s
        .call(
            "Runtime.callFunctionOn",
            json!({
                "objectId": object_id,
                "functionDeclaration": r#"function(){
                    this.scrollIntoView({block:'center'});
                    const r = this.getBoundingClientRect();
                    if (!this.isConnected || (!r.width && !r.height)) return null;
                    return JSON.stringify({x: r.x + r.width/2, y: r.y + r.height/2});
                }"#,
                "returnByValue": true
            }),
        )
        .await?;
    let p = r
        .pointer("/result/value")
        .and_then(Value::as_str)
        .and_then(|v| serde_json::from_str::<Value>(v).ok())
        .ok_or_else(|| anyhow!(
            "量不到元素中心（元素不可见或 ref 已随导航失效）；下一步：重新 await snapshot() 取新 ref"
        ))?;
    Ok((
        p.get("x").and_then(Value::as_f64).unwrap_or(0.0),
        p.get("y").and_then(Value::as_f64).unwrap_or(0.0),
    ))
}

/// 悬停到短 ref 元素中心（#23）：触发 CSS `:hover` 与悬停菜单。
///
/// # Errors
///
/// ref 失效或元素不可见。
pub async fn hover_ref(s: &Session, backend_node_id: i64) -> Result<Value> {
    let (x, y) = node_center(s, backend_node_id).await?;
    hover_at(s, x.round() as i64, y.round() as i64).await
}

/// 双击短 ref 元素（#23）：press/release 两轮，clickCount 递增成双击。
///
/// # Errors
///
/// ref 失效或元素不可见。
pub async fn dblclick_ref(s: &Session, backend_node_id: i64) -> Result<Value> {
    let (x, y) = node_center(s, backend_node_id).await?;
    let (x, y) = (x.round() as i64, y.round() as i64);
    dispatch_input_seq(
        s,
        vec![
            (
                "Input.dispatchMouseEvent",
                json!({"type":"mousePressed","x":x,"y":y,"button":"left","clickCount":1}),
            ),
            (
                "Input.dispatchMouseEvent",
                json!({"type":"mouseReleased","x":x,"y":y,"button":"left","clickCount":1}),
            ),
            (
                "Input.dispatchMouseEvent",
                json!({"type":"mousePressed","x":x,"y":y,"button":"left","clickCount":2}),
            ),
            (
                "Input.dispatchMouseEvent",
                json!({"type":"mouseReleased","x":x,"y":y,"button":"left","clickCount":2}),
            ),
        ],
    )
    .await?;
    Ok(json!(true))
}

/// 拖拽：源 ref 中心按下，分步移到目标 ref 中心松开（#23）。
///
/// 鼠标事件序列实现，覆盖 pointer/mouse 型拖拽（sortable、拖放上传区）；
/// 原生 HTML5 `draggable`（dragstart/dragover/drop 语义）不在序列内，
/// 该类页面走页面侧合成事件或 `Input.dispatchDragEvent`（待后续批）。
///
/// # Errors
///
/// 任一 ref 失效或元素不可见。
pub async fn drag_ref(s: &Session, src_bn: i64, dst_bn: i64) -> Result<Value> {
    let (sx, sy) = node_center(s, src_bn).await?;
    let (dx, dy) = node_center(s, dst_bn).await?;
    let (sx, sy) = (sx.round() as i64, sy.round() as i64);
    let (dx, dy) = (dx.round() as i64, dy.round() as i64);
    let mut seq = vec![
        (
            "Input.dispatchMouseEvent",
            json!({"type":"mouseMoved","x":sx,"y":sy}),
        ),
        (
            "Input.dispatchMouseEvent",
            json!({"type":"mousePressed","x":sx,"y":sy,"button":"left","clickCount":1}),
        ),
    ];
    // 分八步移动：不少 dnd 实现要看 mouseMoved 中间点才认拖拽
    for i in 1..=8 {
        let t = i as f64 / 8.0;
        let mx = sx as f64 + (dx - sx) as f64 * t;
        let my = sy as f64 + (dy - sy) as f64 * t;
        seq.push((
            "Input.dispatchMouseEvent",
            json!({"type":"mouseMoved","x":mx.round() as i64,"y":my.round() as i64}),
        ));
    }
    seq.push((
        "Input.dispatchMouseEvent",
        json!({"type":"mouseReleased","x":dx,"y":dy,"button":"left","clickCount":1}),
    ));
    dispatch_input_seq(s, seq).await?;
    Ok(json!(true))
}

/// 真实按键序列输入（#23）：focus 后逐字符 keyDown(text)+keyUp，
/// contenteditable / ProseMirror 类编辑器需要；与 insertText 模式的
/// [`fill_ref`] 并存。
///
/// # Errors
///
/// ref 失效或 focus 失败。
pub async fn type_ref(s: &Session, backend_node_id: i64, text: &str) -> Result<Value> {
    let object_id = resolve_node_object(s, backend_node_id).await?;
    let _ = s
        .call(
            "Runtime.callFunctionOn",
            json!({ "objectId": object_id, "functionDeclaration": "function(){ this.focus(); }", "returnByValue": true }),
        )
        .await;
    let mut seq = Vec::new();
    for ch in text.chars() {
        let c = ch.to_string();
        seq.push((
            "Input.dispatchKeyEvent",
            json!({ "type": "keyDown", "key": c, "text": c }),
        ));
        seq.push((
            "Input.dispatchKeyEvent",
            json!({ "type": "keyUp", "key": c }),
        ));
    }
    dispatch_input_seq(s, seq).await?;
    Ok(json!(true))
}

/// 导出会话态（#25.3）：cookies 全量加当前页 origin 的 localStorage。
///
/// 多 origin 的 localStorage 列举无 CDP 原语：跨 origin 场景逐个
/// switchTab 到目标页再导，各自合并。
///
/// # Errors
///
/// 未连接或 cookies 读取失败。
pub async fn export_storage_state(s: &Session) -> Result<Value> {
    // Storage.getCookies（browser 级，评审 G4）：Network.getAllCookies 已
    // 废弃，且这一面不需要先开 Network 域
    let cookies = s
        .call("Storage.getCookies", json!({}))
        .await
        .map_err(|e| anyhow!("Storage.getCookies 失败：{e:#}"))
        .map(|r| r.get("cookies").cloned().unwrap_or(json!([])))?;
    let mut origins: Vec<Value> = Vec::new();
    if let Ok(tab) = current_tab(s).await
        && let Some(url) = tab.get("url").and_then(Value::as_str)
        && let Some(origin) = url_origin(url)
    {
        let items = s
            .call(
                "DOMStorage.getDOMStorageItems",
                json!({ "storageId": { "securityOrigin": origin, "isLocalStorage": true } }),
            )
            .await;
        // 回包形 {entries: [[k, v], ...]}，取 /entries（评审 G5 实弹）
        let ls: Vec<Value> = items
            .ok()
            .and_then(|r| r.pointer("/entries").cloned())
            .and_then(|e| e.as_array().cloned())
            .unwrap_or_default()
            .into_iter()
            .filter(|pair| pair.as_array().is_some_and(|kv| kv.len() == 2))
            .map(|kv| json!({ "name": kv[0], "value": kv[1] }))
            .collect();
        origins.push(json!({ "origin": origin, "localStorage": ls }));
    }
    Ok(json!({ "cookies": cookies, "origins": origins }))
}

/// 导入会话态（#25.3）：吃 [`export_storage_state`] 的返回值或其落盘
/// 文件路径串；cookies 走 `Network.setCookies`，localStorage 走
/// `DOMStorage.setDOMStorageItem`。回执带两边计数。
///
/// # Errors
///
/// 未连接、路径读不了或 JSON 解析失败。
pub async fn import_storage_state(s: &Session, arg: &Value) -> Result<Value> {
    let state = match arg {
        Value::String(p) => {
            let text = tokio::fs::read_to_string(p)
                .await
                .map_err(|e| anyhow!("读不了存储态文件 {p}：{e}"))?;
            serde_json::from_str::<Value>(&text)
                .map_err(|e| anyhow!("存储态文件不是合法 JSON：{e}"))?
        }
        other => other.clone(),
    };
    let mut cookie_n = 0u64;
    if let Some(cookies) = state.get("cookies").and_then(Value::as_array) {
        for c in cookies {
            if s.call("Network.setCookies", json!({ "cookies": [c] }))
                .await
                .is_ok()
            {
                cookie_n += 1;
            }
        }
    }
    let mut item_n = 0u64;
    if let Some(origins) = state.get("origins").and_then(Value::as_array) {
        for o in origins {
            let Some(origin) = o.get("origin").and_then(Value::as_str) else {
                continue;
            };
            if let Some(ls) = o.get("localStorage").and_then(Value::as_array) {
                for kv in ls {
                    let (k, v) = (
                        kv.get("name").and_then(Value::as_str).unwrap_or(""),
                        kv.get("value").and_then(Value::as_str).unwrap_or(""),
                    );
                    if k.is_empty() {
                        continue;
                    }
                    if s.call(
                        "DOMStorage.setDOMStorageItem",
                        json!({ "storageId": { "securityOrigin": origin, "isLocalStorage": true }, "key": k, "value": v }),
                    )
                    .await
                    .is_ok()
                    {
                        item_n += 1;
                    }
                }
            }
        }
    }
    Ok(json!({ "cookies": cookie_n, "items": item_n }))
}

/// 从 URL 取 securityOrigin（`scheme://host[:port]`，CDP DOMStorage
/// 口径）；非 http(s) 返回 None。
fn url_origin(url: &str) -> Option<String> {
    let scheme = if url.starts_with("https://") {
        "https"
    } else if url.starts_with("http://") {
        "http"
    } else {
        return None;
    };
    let rest = &url[scheme.len() + 3..];
    let host = rest.split(['/', '?', '#']).next()?;
    if host.is_empty() {
        None
    } else {
        Some(format!("{scheme}://{host}"))
    }
}

/// 视口与 UA 仿真档位（#24）：`{viewport:{width,height}, mobile, userAgent,
/// deviceScaleFactor}` 全可省；省 viewport 只设 UA。`mobile: true` 触发
/// 移动仿真（含 touch），同站常更省 token。
///
/// # Errors
///
/// 未连接或 CDP 覆写失败。
pub async fn emulate(s: &Session, opts: &Value) -> Result<Value> {
    if let Some(ua) = opts.get("userAgent").and_then(Value::as_str) {
        s.call("Emulation.setUserAgentOverride", json!({ "userAgent": ua }))
            .await?;
    }
    if let Some(v) = opts.get("viewport").and_then(Value::as_object) {
        let mobile = opts.get("mobile").and_then(Value::as_bool).unwrap_or(false);
        let mut p = json!({
            "width": v.get("width").and_then(Value::as_i64).unwrap_or(1280),
            "height": v.get("height").and_then(Value::as_i64).unwrap_or(800),
            "deviceScaleFactor": v.get("deviceScaleFactor").and_then(Value::as_f64).unwrap_or(1.0),
            "mobile": mobile,
        });
        if mobile {
            p["screenWidth"] = p["width"].clone();
            p["screenHeight"] = p["height"].clone();
        }
        s.call("Emulation.setDeviceMetricsOverride", p).await?;
    }
    Ok(json!(true))
}

/// 等页面 load 完成：先宽容地等一次 frameNavigated（导航可能已完成，
/// 超时忽略），再等 `document.readyState === 'complete'`。
///
/// 已加载页面立即返回。
///
/// # Errors
///
/// `ms` 内 readyState 不到 complete。
pub async fn wait_load(s: &Session, ms: u64) -> Result<Value> {
    let t0 = std::time::Instant::now();
    s.call("Page.enable", json!({})).await?;
    // 先查 readyState 再谈等待（评审 G5）：已加载页立即返回，不再先进
    // frameNavigated 宽限窗白等 ms/3；导航在途时提交屏障已把页面级调用
    // 闸到提交后，首查读到的就是新文档态
    let deadline = tokio::time::Instant::now() + Duration::from_millis(ms.max(200));
    loop {
        let r = s
            .call(
                "Runtime.evaluate",
                json!({ "expression": "document.readyState", "returnByValue": true }),
            )
            .await;
        if let Ok(resp) = r
            && resp.pointer("/result/value").and_then(Value::as_str) == Some("complete")
        {
            return Ok(
                json!({ "elapsedMs": t0.elapsed().as_millis() as u64, "readyState": "complete" }),
            );
        }
        if tokio::time::Instant::now() >= deadline {
            bail!(
                "waitLoad 超时（{} 秒）：readyState 未到 complete；下一步：waitJs 查具体条件或加大超时（秒口径）",
                ms / 1000
            );
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// 等 network 静默：从调用时刻起观察 `Network.requestWillBeSent` 与
/// `loadingFinished/loadingFailed` 的差值，连续两拍在飞为 0 即静默。
///
/// 起点之前挂着的请求不在观察内（窗口语义，同 bh wait_for_network_idle）。
///
/// # Errors
///
/// `ms` 内未静默。
pub async fn wait_idle(s: &Session, ms: u64) -> Result<Value> {
    let t0 = std::time::Instant::now();
    s.call("Network.enable", json!({})).await?;
    let since = s.last_seq().await;
    let deadline = tokio::time::Instant::now() + Duration::from_millis(ms);
    let mut quiet = 0;
    loop {
        tokio::time::sleep(Duration::from_millis(400)).await;
        let sent = s
            .peek_events_since("Network.requestWillBeSent", since, 100_000)
            .await
            .len();
        let fin = s
            .peek_events_since("Network.loadingFinished", since, 100_000)
            .await
            .len()
            + s.peek_events_since("Network.loadingFailed", since, 100_000)
                .await
                .len();
        let in_flight = sent.saturating_sub(fin);
        quiet = if in_flight == 0 { quiet + 1 } else { 0 };
        if quiet >= 2 {
            return Ok(json!({
                "requests": sent,
                "finished": fin,
                "elapsedMs": t0.elapsed().as_millis() as u64,
            }));
        }
        if tokio::time::Instant::now() >= deadline {
            bail!(
                "waitIdle 超时（{} 秒）：仍有 {in_flight} 个在飞请求；下一步：加大超时（秒口径）或 findEvents 看卡住的是什么请求",
                ms / 1000
            );
        }
    }
}

/// 把 backendNodeId 解析成 Runtime objectId（`DOM.resolveNode`）。
///
/// 节点已不在当前页面（导航/移除）时报错并带重取 ref 的 CTA。
///
/// 元素引用（D35-lite）背景：backendNodeId 锚定真交互，ref 的短名映射在
/// [`crate::js_host`]（每次 `snapshot()` 整表替换），本层只管把
/// backendNodeId 变成 focus/click；引用失效是被动发现的（导航后节点
/// 没了，`DOM.resolveNode` 报错 -> CTA 重新 snapshot）。
pub(crate) async fn resolve_node_object(s: &Session, backend_node_id: i64) -> Result<String> {
    match s
        .call("DOM.resolveNode", json!({ "backendNodeId": backend_node_id }))
        .await
    {
        Ok(v) => v
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| anyhow!(
                "DOM.resolveNode 未回 objectId（backendNodeId {backend_node_id}）；下一步：重新 await snapshot() 取新 ref"
            )),
        Err(e) => Err(anyhow!(
            "ref 已失效（节点不在当前页面）：{e:#}；下一步：重新 await snapshot()（导航后旧 ref 全部作废）"
        )),
    }
}

/// 按短 ref 点击：滚动可见 -> 量视口中心 -> **遮挡命中测试** -> 复用
/// [`click_at`] 的 trusted 鼠标事件。
///
/// 命中测试（吸收 agent-browser 的 blocker 思路）：`elementFromPoint` 看
/// 点击点实际落谁头上，落点与目标无祖孙/label 关联即判被遮挡（consent
/// banner、modal 场景），报遮挡元素描述并拒绝点击：绝不静默点错位置。
/// `clickAt` 是显式「点可见物」，不做此检查。
///
/// # Errors
///
/// ref 失效（节点没了）、取不到中心（不可见）、被遮挡（错误附遮挡元素）、
/// 派发失败。
pub async fn click_ref(s: &Session, backend_node_id: i64) -> Result<Value> {
    click_ref_opts(s, backend_node_id, "left", 1).await
}

/// 按 snapshot 短 ref 参数化点击（#35）：button（left/right/middle/
/// back/forward）与 clickCount（2 即双击语义）；遮挡命中测试与 trusted
/// 派发同 [`click_ref`] 口径。
///
/// # Errors
///
/// 同 [`click_ref`]。
pub async fn click_ref_opts(
    s: &Session,
    backend_node_id: i64,
    button: &str,
    click_count: i64,
) -> Result<Value> {
    validate_button(button)?;
    let object_id = resolve_node_object(s, backend_node_id).await?;
    let r = s
        .call(
            "Runtime.callFunctionOn",
            json!({
                "objectId": object_id,
                "functionDeclaration": r#"function(){
                    this.scrollIntoView({block:'center'});
                    const r = this.getBoundingClientRect();
                    if (!this.isConnected || (!r.width && !r.height)) return null;
                    const el = this;
                    const x = r.x + r.width/2, y = r.y + r.height/2;
                    // 下降进同源 iframe：点在 frame 上时解析到 frame 内元素
                    let d = document, lx = x, ly = y;
                    let hit = d.elementFromPoint(lx, ly);
                    while (hit && (hit.tagName === 'IFRAME' || hit.tagName === 'FRAME')
                           && hit.contentDocument && hit !== el) {
                        const fr = hit.getBoundingClientRect();
                        lx -= fr.x + hit.clientLeft;
                        ly -= fr.y + hit.clientTop;
                        d = hit.contentDocument;
                        hit = d.elementFromPoint(lx, ly);
                    }
                    let blocker = null;
                    if (hit && hit !== el) {
                        const up = (n) => n.parentNode || n.host || (n.getRootNode && n.getRootNode().host) || null;
                        let related = false;
                        for (let n = hit; n; n = up(n)) { if (n === el) { related = true; break; } }
                        if (!related) for (let n = el; n; n = up(n)) { if (n === hit) { related = true; break; } }
                        if (!related) {
                            const hl = hit.closest ? hit.closest('label') : null;
                            if (hl && (hl.control === el || hl.contains(el))) related = true;
                            const elLabel = el.closest ? el.closest('label') : null;
                            if (elLabel && elLabel.contains(hit)) related = true;
                        }
                        if (!related) {
                            blocker = hit.tagName.toLowerCase();
                            if (hit.id) blocker += '#' + hit.id;
                            else if (typeof hit.className === 'string' && hit.className.trim())
                                blocker += '.' + hit.className.trim().split(/\s+/).slice(0, 2).join('.');
                        }
                    }
                    return JSON.stringify({x: x, y: y, blocker: blocker});
                }"#,
                "returnByValue": true
            }),
        )
        .await?;
    let probe = r
        .pointer("/result/value")
        .and_then(Value::as_str)
        .and_then(|v| serde_json::from_str::<Value>(v).ok())
        .ok_or_else(|| anyhow!(
            "clickRef 量不到元素中心（元素不可见，或已随导航/重排失效）；下一步：重新 await snapshot() 取新 ref，或 clickAt(x,y) 手点坐标"
        ))?;
    if let Some(b) = probe
        .get("blocker")
        .and_then(Value::as_str)
        .filter(|b| !b.is_empty())
    {
        bail!(
            "clickRef 目标被遮挡：{b} 盖住了点击点；下一步：先 snapshot() 拿遮挡物的 ref，clickRef 它或关掉它，再重试原目标"
        );
    }
    let x = probe.get("x").and_then(Value::as_f64).unwrap_or(0.0);
    let y = probe.get("y").and_then(Value::as_f64).unwrap_or(0.0);
    click_at_opts(s, x.round() as i64, y.round() as i64, button, click_count).await
}

/// 按短 ref 填输入框：objectId 上 focus -> 探测控件（SELECT/readOnly 拒收
/// 并给 CTA）-> SelectAll+insertText（与 [`fill_input`] 同款，不发 Ctrl+A）
/// -> 同一 objectId 回读严格验证。选择器会随重构漂移，backendNodeId 不会。
///
/// # Errors
///
/// ref 失效、目标不是可填控件、回读不一致（错误附回读值）。
pub async fn fill_ref(s: &Session, backend_node_id: i64, text: &str) -> Result<Value> {
    let object_id = resolve_node_object(s, backend_node_id).await?;
    let meta = s
        .call(
            "Runtime.callFunctionOn",
            json!({
                "objectId": object_id,
                "functionDeclaration": "function(){ this.focus(); if (!this.isConnected || (this.offsetWidth === 0 && this.offsetHeight === 0)) return null; return JSON.stringify({tag: this.tagName, ro: this.readOnly === true}); }",
                "returnByValue": true
            }),
        )
        .await?;
    let m = meta
        .pointer("/result/value")
        .and_then(Value::as_str)
        .and_then(|v| serde_json::from_str::<Value>(v).ok())
        .ok_or_else(|| anyhow!(
            "fillRef 目标不可聚焦（不是表单控件，或已随导航失效）；下一步：重新 await snapshot() 取新 ref，或 fillInput(选择器, 文本)"
        ))?;
    if m.get("tag").and_then(Value::as_str) == Some("SELECT") {
        bail!(
            "fillRef 暂不支持 <select>；下一步：selectOption(ref, \"值或可见 label\")（同一个 ref 即可）"
        );
    }
    if m.get("ro").and_then(Value::as_bool) == Some(true) {
        bail!("fillRef 目标是 readOnly（backendNodeId {backend_node_id}）");
    }
    let mut seq: Vec<(&'static str, Value)> = vec![
        (
            "Input.dispatchKeyEvent",
            json!({ "type": "rawKeyDown", "key": "a", "code": "KeyA", "commands": ["SelectAll"] }),
        ),
        (
            "Input.dispatchKeyEvent",
            json!({ "type": "keyUp", "key": "a", "code": "KeyA" }),
        ),
    ];
    if text.is_empty() {
        seq.push((
            "Input.dispatchKeyEvent",
            json!({ "type": "rawKeyDown", "key": "Backspace", "code": "Backspace" }),
        ));
        seq.push((
            "Input.dispatchKeyEvent",
            json!({ "type": "keyUp", "key": "Backspace", "code": "Backspace" }),
        ));
    } else {
        seq.push(("Input.insertText", json!({ "text": text })));
    }
    dispatch_input_seq(s, seq).await?;
    let read = s
        .call(
            "Runtime.callFunctionOn",
            json!({
                "objectId": object_id,
                "functionDeclaration": "function(){ return this.value; }",
                "returnByValue": true
            }),
        )
        .await?;
    let got = read
        .pointer("/result/value")
        .and_then(Value::as_str)
        .unwrap_or("");
    if got != text {
        bail!(
            "fillRef 回读不一致：期望 {text:?} 实得 {got:?}（backendNodeId {backend_node_id}）；下一步：检查是否有 JS 覆写或格式化输入，或重新 snapshot() 取新 ref"
        );
    }
    Ok(json!(text))
}

/// 当前页存 PDF（`Page.printToPDF`，`printBackground`+`preferCSSPageSize`），
/// 仅无头 chrome 支持（Chromium 限制）。
///
/// 路径缺省落 `<state>/pdf-<ts>.pdf`，回 `{path,bytes}`：screenshot 的
/// 姊妹件，agent 存档页面用。
///
/// # Errors
///
/// 有头 chrome（CTP 拒绝）、PDF 生成或写盘失败。
pub async fn pdf(s: &Session, path: Option<&str>) -> Result<Value> {
    let r = s
        .call(
            "Page.printToPDF",
            json!({ "printBackground": true, "preferCSSPageSize": true }),
        )
        .await
        .map_err(|e| anyhow!(
            "pdf 失败：{e:#}（Page.printToPDF 仅无头 chrome 支持）；下一步：browse down 后 browse up --headless 再试"
        ))?;
    let data = r
        .get("data")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("printToPDF 未回 data"))?;
    let bytes = crate::js_host::base64_decode(data)?;
    let path = match path {
        Some(p) => std::path::PathBuf::from(p),
        None => {
            let dir = crate::paths::state_dir().join("pdfs");
            tokio::fs::create_dir_all(&dir).await.ok();
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            dir.join(format!("pdf-{ts}.pdf"))
        }
    };
    let display = path.display().to_string();
    tokio::fs::write(&path, &bytes).await?;
    Ok(json!({ "path": display, "bytes": bytes.len() }))
}

/// 按短 ref 选下拉框选项：value 或可见 label 匹配，设值并派发 input+change
/// （不发鼠标事件，确定性路径），回 `{value,label}`。
///
/// 选择器版的入口是 `fillInput` 的 SELECT CTA（指向本函数先 snapshot 取 ref）。
///
/// # Errors
///
/// ref 失效、目标不是 `<select>`、没有匹配选项（错误附全部可选 value）。
pub async fn select_option(s: &Session, backend_node_id: i64, value: &str) -> Result<Value> {
    let object_id = resolve_node_object(s, backend_node_id).await?;
    let r = s
        .call(
            "Runtime.callFunctionOn",
            json!({
                "objectId": object_id,
                "functionDeclaration": r#"function(v){
                    const el = this;
                    if (!el || el.tagName !== 'SELECT') return JSON.stringify({notSelect: true});
                    const opt = Array.from(el.options)
                        .find(o => o.value === v || o.label === v || (o.textContent || '').trim() === v);
                    if (!opt) return JSON.stringify({miss: Array.from(el.options).map(o => o.value)});
                    el.value = opt.value;
                    el.dispatchEvent(new Event('input', {bubbles: true}));
                    el.dispatchEvent(new Event('change', {bubbles: true}));
                    return JSON.stringify({ok: true, value: opt.value, label: opt.label});
                }"#,
                "arguments": [{ "value": value }],
                "returnByValue": true
            }),
        )
        .await?;
    let probe = r
        .pointer("/result/value")
        .and_then(Value::as_str)
        .and_then(|v| serde_json::from_str::<Value>(v).ok())
        .ok_or_else(|| anyhow!(
            "selectOption 目标不可用（ref 已随导航失效或不可交互）；下一步：重新 await snapshot() 取新 ref"
        ))?;
    if probe.get("notSelect").is_some() {
        bail!(
            "selectOption 目标不是 <select>（backendNodeId {backend_node_id}）；下一步：换正确的 ref，或用 Runtime.evaluate 设值"
        );
    }
    if let Some(miss) = probe.get("miss").and_then(Value::as_array) {
        let opts: Vec<String> = miss
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        bail!(
            "selectOption 没有匹配选项 {value:?}；可选 value：{}；下一步：用其中之一（也接受可见 label）",
            opts.join(" / ")
        );
    }
    Ok(json!({
        "value": probe.get("value"),
        "label": probe.get("label"),
    }))
}

/// 派发一串 Input 域调用，带两条挂起自愈路径：
/// - 页面 JS 对话框开着：Input 被它挂起，立即报「先处理对话框」CTA，
///   不烧超时（状态来自 [`cdp::Session::pending_dialog`]，route 层截获）。
/// - `cdp timeout`（从未激活的后台 tab 收 Input 的典型症状）：
///   `Target.activateTarget` 后整串重试一次；bh 内置激活重试同款，
///   只在挂起时自愈，不主动抢用户前台。
async fn dispatch_input_seq(s: &Session, seq: Vec<(&'static str, Value)>) -> Result<()> {
    if s.pending_dialog().await.is_some() {
        bail!(
            "页面有未处理的 confirm/prompt 对话框（Input 会被它挂起）；下一步：return await dialogStatus() 看内容，再 await dialogAccept() 或 await dialogDismiss()"
        );
    }
    match run_input_seq(s, &seq).await {
        Ok(()) => Ok(()),
        Err(first) if format!("{first:#}").contains("cdp timeout") => {
            if s.pending_dialog().await.is_some() {
                bail!(
                    "交互触发了页面对话框（后续 Input 被它挂起）；下一步：return await dialogStatus() 看内容，再 await dialogAccept() 或 await dialogDismiss()"
                );
            }
            if let Some(t) = s.active_target().await {
                let _ = s
                    .call("Target.activateTarget", json!({ "targetId": t }))
                    .await;
            }
            run_input_seq(s, &seq)
                .await
                .map_err(|second| second.context(format!("激活重试后仍失败（首次：{first:#}）")))
        }
        Err(e) => Err(e),
    }
}

/// Input 派发通常瞬时完成；单条 8 秒短超时让「对话框/后台 tab 挂起」
/// 尽快落进自愈路径，而不是干等全量 30 秒。
const INPUT_DISPATCH_TIMEOUT: Duration = Duration::from_secs(8);

async fn run_input_seq(s: &Session, seq: &[(&'static str, Value)]) -> Result<()> {
    for (method, params) in seq {
        match tokio::time::timeout(INPUT_DISPATCH_TIMEOUT, s.call(method, params.clone())).await {
            Ok(r) => {
                r?;
            }
            Err(_) => return Err(anyhow!("cdp timeout (input {method})")),
        }
    }
    Ok(())
}

async fn tab_brief(s: &Session, target_id: &str) -> Result<Value> {
    let tabs = s.list_page_targets().await?;
    Ok(tabs
        .iter()
        .find(|t| t.target_id == target_id)
        .map(|t| json!({ "targetId": t.target_id, "title": t.title, "url": t.url, "own": t.own }))
        .unwrap_or(json!({ "targetId": target_id, "title": "", "url": "" })))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 修饰键解析（#23）：位值、别名与末位键；坏写法带 CTA。
    #[test]
    fn modifier_parsing() {
        assert_eq!(parse_modifiers("Control+a").unwrap(), (2, "a".into()));
        assert_eq!(
            parse_modifiers("Ctrl+Alt+Delete").unwrap(),
            (3, "Delete".into())
        );
        assert_eq!(
            parse_modifiers("Shift+Meta+ArrowLeft").unwrap(),
            (12, "ArrowLeft".into())
        );
        let e = parse_modifiers("Hyper+x").unwrap_err().to_string();
        assert!(e.contains("未知修饰键") && e.contains("下一步"), "{e}");
        let e = parse_modifiers("Control+").unwrap_err().to_string();
        assert!(e.contains("不完整"), "{e}");
    }

    /// securityOrigin 解析（#25.3）：scheme 保留，非 http(s) 与空 host 拒。
    #[test]
    fn origin_parsing() {
        assert_eq!(
            url_origin("https://a.test:8443/x?y#z").unwrap(),
            "https://a.test:8443"
        );
        assert_eq!(url_origin("http://b.test/").unwrap(), "http://b.test");
        assert!(url_origin("data:text/html,x").is_none());
        assert!(url_origin("about:blank").is_none());
    }
}
