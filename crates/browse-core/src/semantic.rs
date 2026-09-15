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
    s.use_target(&id).await?;
    if let Some(u) = url {
        s.call("Page.navigate", json!({ "url": u })).await?;
        // target 列表的 title/url 在 load 前是滞后的 about:blank，等完再取简表
        let _ = wait_load(s, 8000).await;
    }
    tab_brief(s, &id).await
}

/// 切换活动路由到既有 tab（不改 Chrome 可见前景），返回该 tab 简表。
///
/// # Errors
///
/// 未连接或 attach 失败（targetId 不存在）。
pub async fn switch_tab(s: &Session, target_id: &str) -> Result<Value> {
    s.use_target(target_id).await?;
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

/// 关 tab；缺省关当前活动 tab。守卫层只放行本会话自建 tab——用户 tab 一律拒绝。
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

/// 真点击：`Input.dispatchMouseEvent` pressed+released 于视口坐标 (x,y)。
/// 事件是 trusted 的；坐标命中的是当前可见物。不自动激活 tab（人机共存）。
///
/// # Errors
///
/// 未连接或派发失败。从未激活的后台 tab 可能挂起（30s 超时），届时按错误
/// CTA 先 `session.Target.activateTarget({targetId})`。
pub async fn click_at(s: &Session, x: i64, y: i64) -> Result<Value> {
    dispatch_input_seq(
        s,
        vec![
            ("Input.dispatchMouseEvent", json!({ "type": "mousePressed", "x": x, "y": y, "button": "left", "clickCount": 1 })),
            ("Input.dispatchMouseEvent", json!({ "type": "mouseReleased", "x": x, "y": y, "button": "left", "clickCount": 1 })),
        ],
    )
    .await?;
    Ok(json!(true))
}

/// 按 CSS 选择器填输入框：focus -> 全选（commands，不发 Ctrl+A）-> 可选
/// Backspace 清空 -> `Input.insertText` -> 回读严格验证。只支持文本类控件；
/// `<select>` 用 `Runtime.evaluate` 设 value 并派发 change 事件（CTA 给写法）。
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

/// 按一个键：`Input.dispatchKeyEvent` keyDown(+text)+keyUp。Enter 的 text
/// 是 `\r`（CDP 契约）；可打印单字符带自身为 text。
///
/// # Errors
///
/// 未连接或派发失败。
pub async fn press_key(s: &Session, key: &str) -> Result<Value> {
    let text = match key {
        "Enter" => "\r",
        k if k.chars().count() == 1 => k,
        _ => "",
    };
    let mut down = json!({ "type": "keyDown", "key": key });
    if !text.is_empty() {
        down["text"] = json!(text);
    }
    dispatch_input_seq(
        s,
        vec![
            ("Input.dispatchKeyEvent", down),
            (
                "Input.dispatchKeyEvent",
                json!({ "type": "keyUp", "key": key }),
            ),
        ],
    )
    .await?;
    Ok(json!(true))
}

/// 等 load：先宽容地等一次 frameNavigated（导航可能已完成，超时忽略），
/// 再等 `document.readyState === 'complete'`。已加载页面立即返回。
///
/// # Errors
///
/// `ms` 内 readyState 不到 complete。
pub async fn wait_load(s: &Session, ms: u64) -> Result<Value> {
    let t0 = std::time::Instant::now();
    s.call("Page.enable", json!({})).await?;
    let nav_budget = ms / 3;
    if let Ok(_ev) = s.wait_for("Page.frameNavigated", nav_budget.max(200)).await {
        // 新导航在路上，交给 readyState 收尾
    }
    let remain = ms.saturating_sub(t0.elapsed().as_millis() as u64).max(200);
    let deadline = tokio::time::Instant::now() + Duration::from_millis(remain);
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
                "waitLoad 超时（{ms}ms）：readyState 未到 complete；下一步：waitJs 查具体条件或加大超时"
            );
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// 等 network 静默：从调用时刻起观察 `Network.requestWillBeSent` 与
/// `loadingFinished/loadingFailed` 的差值，连续两拍在飞为 0 即静默。
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
                "waitIdle 超时（{ms}ms）：仍有 {in_flight} 个在飞请求；下一步：加大超时或 findEvents 看卡住的是什么请求"
            );
        }
    }
}

/// 派发一串 Input 域调用；撞 `cdp timeout`（从未激活的后台 tab 收 Input
/// 的典型症状）时 `Target.activateTarget` 后整串重试一次——bh 内置激活
/// 重试同款：只在挂起时自愈，不主动抢用户前台。
/// 元素引用（D35-lite）：backendNodeId 锚定的真交互。ref 的短名映射在
/// [`crate::js_host`]（每次 `snapshot()` 整表替换）；本层只管把
/// backendNodeId 变成 focus/click。引用失效是被动发现的：导航后节点
/// 没了，`DOM.resolveNode` 报错 -> CTA 重新 snapshot。
///
/// backendNodeId -> Runtime objectId（`DOM.resolveNode`）。节点已不在
/// 当前页面（导航/移除）时报错并带重取 ref 的 CTA。
async fn resolve_node_object(s: &Session, backend_node_id: i64) -> Result<String> {
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
/// [`click_at`] 的 trusted 鼠标事件。命中测试（吸收 agent-browser 的
/// blocker 思路）：`elementFromPoint` 看点击点实际落谁头上，落点与目标
/// 无祖孙/label 关联即判被遮挡（consent banner、modal 场景），报遮挡
/// 元素描述并拒绝点击——绝不静默点错位置。`clickAt` 是显式「点可见物」，
/// 不做此检查。
///
/// # Errors
///
/// ref 失效（节点没了）、取不到中心（不可见）、被遮挡（错误附遮挡元素）、
/// 派发失败。
pub async fn click_ref(s: &Session, backend_node_id: i64) -> Result<Value> {
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
    click_at(s, x.round() as i64, y.round() as i64).await
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

/// 当前页存 PDF（`Page.printToPDF`，`printBackground`+`preferCSSPageSize`）。
/// 仅无头 chrome 支持（Chromium 限制）。路径缺省落 `<state>/pdf-<ts>.pdf`，
/// 回 `{path,bytes}`——screenshot 的姊妹件，agent 存档页面用。
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
/// （不发鼠标事件，确定性路径）。回 `{value,label}`。选择器版的入口是
/// `fillInput` 的 SELECT CTA（指向本函数先 snapshot 取 ref）。
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

/// 派发一串 Input 域调用。两个挂起自愈路径：
/// - 页面 JS 对话框开着：Input 被它挂起——立即报「先处理对话框」CTA，
///   不烧超时（状态来自 [`cdp::Session::pending_dialog`]，route 层截获）。
/// - `cdp timeout`（从未激活的后台 tab 收 Input 的典型症状）：
///   `Target.activateTarget` 后整串重试一次——bh 内置激活重试同款，
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
