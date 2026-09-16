//! 真 chrome 端到端（本机 clean-chrome）。门控：`BROWSE_E2E=1`，CI 无浏览器跳过。
//!
//! 覆盖：spawn headless 引擎（端口态与管道态各一）-> 方言 navigate data: URL
//! -> Runtime.evaluate 读 title -> down 只杀自起实例。

use browse_core::{Engine, EngineSpec, JsHost};
use serde_json::{Value, json};

/// 串行化两通道测试：engine-profile 是独占资源（chrome 单实例锁）。
static SEQ: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn gated() -> bool {
    // 跑法：BROWSE_E2E=1 BROWSE_NO_ATTACH=1 cargo test …
    // （NO_ATTACH 跳过附着探测：本机 9222 上可能开着用户自己的
    // clean-chrome，e2e 要的是自起隔离实例，不许撞上）
    std::env::var("BROWSE_E2E").ok().as_deref() == Some("1")
}

async fn clean_profile() {
    // 先收割上次被杀测试留下的引擎 chrome（只认 browse-rs 的引擎 profile，
    // 不碰用户浏览器），否则单实例 profile 锁让本次 spawn 连坐挂死
    #[cfg(windows)]
    let reap = || {
        let _ = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "Get-Process chrome -ErrorAction SilentlyContinue | Where-Object {$_.Path -like '*browse-rs*'} | Stop-Process -Force",
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    };
    #[cfg(unix)]
    let reap = || {
        let _ = std::process::Command::new("pkill")
            .args(["-9", "-f", "\\.browse-rs/engine-profile"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    };
    let _ = tokio::task::spawn_blocking(reap).await;
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    let dir = browse_core::paths::engine_profile_dir();
    let _ = tokio::task::spawn_blocking(move || {
        std::fs::remove_dir_all(dir).ok();
    })
    .await;
}

async fn exercise(engine: &Engine, host: &JsHost, expect_channel: &str) {
    match engine.source().await {
        browse_core::EngineSource::Spawned { channel, .. } => {
            assert_eq!(channel, expect_channel, "通道不符");
        }
        other => panic!("预期 Spawned 来源，实际 {other:?}"),
    }

    let nav = host
        .eval_snippet(
            r#"await session.Page.navigate({url:"data:text/html,<title>browse-e2e</title><h1>ok</h1>"})"#,
        )
        .await
        .expect("navigate");
    assert!(nav.get("frameId").is_some(), "navigate 应回 frameId: {nav}");

    let title = host
        .eval_snippet(
            r#"return (await session.Runtime.evaluate({expression:"document.title", returnByValue:true})).result.value"#,
        )
        .await
        .expect("evaluate");
    assert_eq!(title, json!("browse-e2e"));

    // peekEvents 非破坏：frameStartedLoading 一定先于 frameNavigated 落缓冲，
    // 等到后者时前者必在；peek 它不消费，waitFor 仍取得到
    host.eval_snippet("await session.Page.enable({})")
        .await
        .expect("Page.enable");
    host.eval_snippet(
        r#"await session.Page.navigate({url:"data:text/html,<title>browse-e2e</title>"})"#,
    )
    .await
    .expect("再导航");
    host.eval_snippet(r#"await session.waitFor("Page.frameNavigated", undefined, 15000)"#)
        .await
        .expect("waitFor frameNavigated");
    let peeked = host
        .eval_snippet(r#"return await session.peekEvents("Page.frameStartedLoading", 3)"#)
        .await
        .expect("peekEvents");
    assert!(
        peeked.as_array().is_some_and(|a| !a.is_empty()),
        "peek 应见到 frameStartedLoading: {peeked}"
    );
    // seq 盖戳 + 增量轮询：记下当前游标，再导航一次，since 只见新事件
    let cursor = peeked
        .as_array()
        .and_then(|a| a.last())
        .and_then(|e| e.get("seq").and_then(serde_json::Value::as_u64))
        .expect("事件应带 seq");
    host.eval_snippet(
        r#"await session.Page.navigate({url:"data:text/html,<title>browse-e2e-2</title>"})"#,
    )
    .await
    .expect("第三次导航");
    let fresh = host
        .eval_snippet(&format!(
            r#"return await session.peekEventsSince("Page.frameStartedLoading", {cursor}, 5)"#
        ))
        .await
        .expect("peekEventsSince");
    let arr = fresh.as_array().expect("数组");
    assert!(!arr.is_empty(), "since 之后应有新事件");
    assert!(
        arr.iter().all(|e| {
            e.get("seq")
                .and_then(serde_json::Value::as_u64)
                .is_some_and(|s| s > cursor)
        }),
        "since 返回的事件 seq 必须全部大于游标"
    );
    host.eval_snippet(r#"await session.waitFor("Page.frameStartedLoading", undefined, 15000)"#)
        .await
        .expect("peek 之后 waitFor 仍取得到（非破坏）");

    // waitJs：页内 800ms 后置真，谓词轮询等到
    host.eval_snippet(
        r#"await session.Page.navigate({url:"data:text/html,<script>setTimeout(() => window.done = 42, 800)</script>"})"#,
    )
    .await
    .expect("导航到 waitJs 页");
    let waited = host
        .eval_snippet(r#"await session.waitJs("window.done", 8000)"#)
        .await
        .expect("waitJs");
    assert_eq!(waited, json!(42), "waitJs 应返回真值本身");

    // snapshot：AX 树精简节点，含按钮
    host.eval_snippet(
        r#"await session.Page.navigate({url:"data:text/html,<title>ax</title><button onclick='window.go=5'>GoGo</button><input value=\"hi\">"})"#,
    )
    .await
    .expect("导航到 snapshot 页");
    let snap = host
        .eval_snippet("return await snapshot()")
        .await
        .expect("snapshot");
    assert_eq!(
        snap.pointer("/title"),
        Some(&json!("ax")),
        "snapshot 带 title"
    );
    let nodes = snap.get("nodes").and_then(Value::as_array).expect("nodes");
    assert!(
        nodes
            .iter()
            .any(|n| n.get("role") == Some(&json!("button"))
                && n.get("name") == Some(&json!("GoGo"))),
        "snapshot 应含 role=button name=GoGo: {snap}"
    );

    eprintln!("[e2e] 元素引用 D35-lite 开始");
    // ---- 元素引用（D35-lite）----
    // fillRef：按 snapshot 短 ref 填输入框（value=hi 的那个），回读即验
    let input_ref = nodes
        .iter()
        .find(|n| n.get("value") == Some(&json!("hi")))
        .and_then(|n| n.get("ref"))
        .and_then(Value::as_str)
        .expect("输入框节点应带 ref")
        .to_string();
    let filled_ref = host
        .eval_snippet(&format!(r#"await fillRef("{input_ref}", "ref filled")"#))
        .await
        .expect("fillRef");
    assert_eq!(filled_ref, json!("ref filled"), "fillRef 返回回读值");
    // clickRef：按 ref 点按钮，页内副作用可观察
    let btn_ref = nodes
        .iter()
        .find(|n| n.get("role") == Some(&json!("button")))
        .and_then(|n| n.get("ref"))
        .and_then(Value::as_str)
        .expect("按钮节点应带 ref")
        .to_string();
    host.eval_snippet(&format!(r#"await clickRef("{btn_ref}")"#))
        .await
        .expect("clickRef");
    let go = host
        .eval_snippet(r#"await session.waitJs("window.go", 3000)"#)
        .await
        .expect("clickRef 副作用");
    assert_eq!(go, json!(5));
    // 遮挡守卫：全屏盖板盖住按钮 -> clickRef 拒点并报遮挡物；撤盖板后放行
    host.eval_snippet(
        r#"await session.Runtime.evaluate({expression:"const d = document.createElement('div'); d.id = 'cover'; d.style = 'position:fixed;inset:0;z-index:9;background:rgb(0,0,0)'; document.body.appendChild(d)"})"#,
    )
    .await
    .expect("加盖板");
    let blocked = host
        .eval_snippet(&format!(r#"await clickRef("{btn_ref}")"#))
        .await;
    let blocked_msg = format!("{blocked:#?}");
    assert!(
        blocked.is_err() && blocked_msg.contains("被遮挡") && blocked_msg.contains("cover"),
        "盖板应触发遮挡拒绝并报遮挡物: {blocked_msg}"
    );
    host.eval_snippet(
        r#"await session.Runtime.evaluate({expression:"document.getElementById('cover').remove()"})"#,
    )
    .await
    .expect("撤盖板");
    host.eval_snippet(&format!(r#"await clickRef("{btn_ref}")"#))
        .await
        .expect("撤盖板后 clickRef 放行");
    // 未知 ref：错误带「先 snapshot」CTA
    let unknown = host.eval_snippet(r#"await clickRef("e9999")"#).await;
    assert!(
        unknown.is_err() && format!("{unknown:#?}").contains("snapshot"),
        "未知 ref 应带 CTA: {unknown:?}"
    );
    // 导航后旧 ref 失效：报错引导重新 snapshot（不许静默点错位置）
    host.eval_snippet(r#"await session.Page.navigate({url:"data:text/html,<title>gone</title>"})"#)
        .await
        .expect("导航离开");
    let stale = host
        .eval_snippet(&format!(r#"await clickRef("{btn_ref}")"#))
        .await;
    assert!(
        stale.is_err() && format!("{stale:#?}").contains("snapshot"),
        "导航后旧 ref 应失效并带重取 CTA: {stale:?}"
    );

    // screenshot：存文件、字节数为正、清场
    let shot = host
        .eval_snippet("return await screenshot()")
        .await
        .expect("screenshot");
    let path = shot
        .get("path")
        .and_then(Value::as_str)
        .expect("path")
        .to_string();
    let bytes = shot.get("bytes").and_then(Value::as_u64).unwrap_or(0);
    assert!(bytes > 0, "截图应有内容: {shot}");
    let meta = tokio::fs::metadata(&path).await.expect("截图文件应在");
    assert!(meta.len() > 0);
    tokio::fs::remove_file(&path).await.ok();

    eprintln!("[e2e] pdf/select/dialog 开始");
    // pdf：无头专属，存盘字节为正，%PDF 头
    let doc = host.eval_snippet("return await pdf()").await.expect("pdf");
    let pdf_path = doc
        .get("path")
        .and_then(Value::as_str)
        .expect("pdf path")
        .to_string();
    let pdf_bytes = doc.get("bytes").and_then(Value::as_u64).unwrap_or(0);
    // 空页 PDF 本来就只有几百字节，真校验靠 %PDF 头
    assert!(pdf_bytes > 300, "PDF 应有内容: {doc}");
    let head = tokio::fs::read(&pdf_path).await.expect("pdf 文件应在");
    assert!(head.starts_with(b"%PDF"), "应是 PDF 文件头");
    tokio::fs::remove_file(&pdf_path).await.ok();

    // selectOption：value 与 label 双路径 + 未命中 CTA + 回读
    host.eval_snippet(
        r#"await session.Page.navigate({url:"data:text/html,<select id='s'><option value='a'>Alpha</option><option value='b'>Beta</option></select>"})"#,
    )
    .await
    .expect("导航 select 页");
    let ssn = host
        .eval_snippet("return await snapshot()")
        .await
        .expect("select snapshot");
    let sel_ref = ssn
        .get("nodes")
        .and_then(Value::as_array)
        .expect("nodes")
        .iter()
        .find(|n| n.get("role") == Some(&json!("combobox")))
        .and_then(|n| n.get("ref"))
        .and_then(Value::as_str)
        .expect("combobox 应带 ref")
        .to_string();
    let picked = host
        .eval_snippet(&format!(
            r#"return await selectOption("{sel_ref}", "Beta")"#
        ))
        .await
        .expect("selectOption label 路径");
    assert_eq!(picked.pointer("/value"), Some(&json!("b")));
    assert_eq!(picked.pointer("/label"), Some(&json!("Beta")));
    let readback = host
        .eval_snippet(
            r#"return (await session.Runtime.evaluate({expression:"document.getElementById('s').value", returnByValue:true})).result.value"#,
        )
        .await
        .expect("select 回读");
    assert_eq!(readback, json!("b"), "selectOption 应真实改值");
    let miss = host
        .eval_snippet(&format!(r#"await selectOption("{sel_ref}", "nope")"#))
        .await;
    assert!(
        miss.is_err() && format!("{miss:#?}").contains("可选 value"),
        "未命中应列可选值: {miss:?}"
    );

    // 对话框：alert 自动接受（不阻塞后续 evaluate）；confirm 显式处理
    let dlg_t0 = std::time::Instant::now();
    let mark = |s: &str| eprintln!("[e2e] dialog {s} +{:?}", dlg_t0.elapsed());
    host.eval_snippet(
        r#"await session.Page.navigate({url:"data:text/html,<button onclick='alert(\"hi\")'>A</button><button onclick='confirm(\"sure?\")'>C</button>"})"#,
    )
    .await
    .expect("导航 dialog 页");
    mark("nav");
    let dsn = host
        .eval_snippet("return await snapshot()")
        .await
        .expect("dialog snapshot");
    mark("snapshot");
    let refs: Vec<String> = dsn
        .get("nodes")
        .and_then(Value::as_array)
        .expect("nodes")
        .iter()
        .filter(|n| n.get("role") == Some(&json!("button")))
        .filter_map(|n| n.get("ref").and_then(Value::as_str).map(str::to_string))
        .collect();
    assert!(refs.len() >= 2, "应有两个按钮 ref: {dsn}");
    // alert：clickRef 后 watcher 1 秒内自动接受，evaluate 不被拖死
    host.eval_snippet(&format!(r#"await clickRef("{}")"#, refs[0]))
        .await
        .expect("点 alert 按钮");
    mark("click-alert");
    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
    let t0 = std::time::Instant::now();
    host.eval_snippet(
        r#"return await session.Runtime.evaluate({expression:"1+1", returnByValue:true})"#,
    )
    .await
    .expect("alert 被自动接受后 evaluate 应立刻可用");
    mark("evaluate-after-alert");
    assert!(t0.elapsed().as_secs() < 10, "alert 不应阻塞 evaluate");
    // confirm：点开（Ok 或 8s 短超时后「对话框 CTA」都算达阵；pressed
    // 已送达、对话框已开）-> dialogStatus 可见 -> 再点被快失败拦 -> 收掉
    let opened = host
        .eval_snippet(&format!(r#"await clickRef("{}")"#, refs[1]))
        .await;
    let opened_ok = opened.is_ok() || format!("{opened:#?}").contains("dialogStatus");
    assert!(opened_ok, "confirm 点击应成功或触发对话框 CTA: {opened:?}");
    mark("click-confirm");
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    let st = host
        .eval_snippet("return await dialogStatus()")
        .await
        .expect("dialogStatus");
    mark("status");
    assert_eq!(st.pointer("/open"), Some(&json!(true)), "{st}");
    assert_eq!(st.pointer("/type"), Some(&json!("confirm")), "{st}");
    let held = host
        .eval_snippet(&format!(r#"await clickRef("{}")"#, refs[0]))
        .await;
    mark("held-click");
    assert!(
        held.is_err() && format!("{held:#?}").contains("dialogAccept"),
        "对话框未处理时交互应快失败并带 CTA: {held:?}"
    );
    host.eval_snippet("await dialogAccept()")
        .await
        .expect("dialogAccept");
    mark("accept");
    let st2 = host
        .eval_snippet("return await dialogStatus()")
        .await
        .expect("dialogStatus2");
    mark("status2");
    assert_eq!(st2.pointer("/open"), Some(&json!(false)), "{st2}");

    eprintln!("[e2e] network route 开始");
    // mock：拦截即本地应答（假域名也行，请求根本不出门）
    host.eval_snippet(
        r#"await routeMock("http://mock.test/api*", "{\"ok\":1}", {contentType: "application/json"})"#,
    )
    .await
    .expect("routeMock");
    host.eval_snippet(
        r#"await session.Page.navigate({url:"data:text/html,<script>fetch('http://mock.test/api/data').then(r => r.text()).then(t => window.got = t).catch(e => window.err = String(e))</script>"})"#,
    )
    .await
    .expect("导航 mock 页");
    let got = host
        .eval_snippet(r#"await session.waitJs("window.got", 8000)"#)
        .await
        .expect("mock 应答应到达");
    assert_eq!(got, json!("{\"ok\":1}"), "mock body 应原样到达: {got}");
    // block：命中的请求直接失败
    host.eval_snippet(r#"await routeBlock("http://block.test/*")"#)
        .await
        .expect("routeBlock");
    host.eval_snippet(
        r#"await session.Page.navigate({url:"data:text/html,<script>fetch('http://block.test/x').then(() => window.ok = 1).catch(e => window.err = String(e))</script>"})"#,
    )
    .await
    .expect("导航 block 页");
    let err = host
        .eval_snippet(r#"await session.waitJs("window.err", 8000)"#)
        .await
        .expect("block 应让 fetch 失败");
    assert!(
        err.as_str()
            .is_some_and(|s| s.to_lowercase().contains("block") || s.contains("Failed")),
        "失败原因应是拦截: {err}"
    );
    // clear：恢复直连（mock 域名不再被应答，直接网络层失败）
    host.eval_snippet("await routeClear()")
        .await
        .expect("routeClear");

    eprintln!("[e2e] 录制开始");
    // 录制：startScreencast 帧流落盘；导航触发重绘产帧
    let rec = host
        .eval_snippet("return await recordStart()")
        .await
        .expect("recordStart");
    assert!(rec.get("dir").is_some(), "recordStart 回 dir: {rec}");
    host.eval_snippet(
        r#"await session.Page.navigate({url:"data:text/html,<h1 style='background:rgb(255,0,0);height:100vh'>one</h1>"})"#,
    )
    .await
    .expect("录制中导航 1");
    host.eval_snippet(
        r#"await session.waitJs("document.body && document.body.innerText.includes('one')", 5000)"#,
    )
    .await
    .expect("等一屏就绪");
    host.eval_snippet(
        r#"await session.Page.navigate({url:"data:text/html,<h2 style='background:rgb(0,255,0)'>two</h2>"})"#,
    )
    .await
    .expect("录制中导航 2");
    let stopped = host
        .eval_snippet("return await recordStop()")
        .await
        .expect("recordStop");
    let frames = stopped.get("frames").and_then(Value::as_u64).unwrap_or(0);
    assert!(frames >= 1, "应录到至少一帧: {stopped}");
    let rec_dir = stopped
        .get("dir")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let on_disk = std::fs::read_dir(&rec_dir)
        .map(|rd| rd.filter_map(|e| e.ok()).count() as u64)
        .unwrap_or(0);
    assert_eq!(on_disk, frames, "盘上帧文件数应与计数一致（dir {rec_dir}）");
    let _ = tokio::fs::remove_dir_all(&rec_dir).await;
    // 没在录时 recordStop：错误带 CTA
    let no_rec = host.eval_snippet("await recordStop()").await;
    assert!(
        no_rec.is_err() && format!("{no_rec:#?}").contains("recordStart"),
        "空停应带开录 CTA: {no_rec:?}"
    );

    eprintln!("[e2e] 语义面开始");
    // ---- 语义层近期面 ----
    // tab 族：newTab -> currentTab -> switchTab 往返 -> closeTab 自建
    let t2 = host
        .eval_snippet(r#"await newTab("data:text/html,<title>tab-two</title><input id=\"q\" value=\"old\">")"#)
        .await
        .expect("newTab");
    assert_eq!(
        t2.pointer("/title"),
        Some(&json!("tab-two")),
        "newTab 后活动 tab 即新 tab: {t2}"
    );
    let cur = host
        .eval_snippet("return await currentTab()")
        .await
        .expect("currentTab");
    eprintln!("[e2e] newTab+currentTab ok");
    assert_eq!(cur.pointer("/targetId"), t2.pointer("/targetId"));
    // 列表顺序跨平台不保证（Linux 的 Target.getTargets 顺序与 Win/mac 不同），
    // 一律按 targetId 挑：t2 是刚开的，t1 是「不是 t2 的那个」
    let t2_id = t2
        .pointer("/targetId")
        .and_then(Value::as_str)
        .expect("t2 应带 targetId")
        .to_string();
    let tabs = host
        .eval_snippet("return await listPageTargets()")
        .await
        .expect("列表");
    let t1_id = tabs
        .as_array()
        .expect("tabs 数组")
        .iter()
        .find(|t| t.pointer("/targetId").and_then(Value::as_str) != Some(&t2_id))
        .and_then(|t| t.pointer("/targetId"))
        .and_then(Value::as_str)
        .expect("应存在另一个 tab")
        .to_string();
    host.eval_snippet(&format!(r#"await switchTab("{t1_id}")"#))
        .await
        .expect("switchTab");
    let back = host
        .eval_snippet("return await currentTab()")
        .await
        .expect("currentTab2");
    eprintln!("[e2e] switchTab 往返 ok");
    assert_eq!(back.pointer("/targetId"), Some(&json!(t1_id)));
    host.eval_snippet(&format!(r#"await switchTab("{t2_id}")"#))
        .await
        .expect("切回 t2");

    eprintln!("[e2e] fillInput 开始");
    // fillInput：清空旧值 + 回读验证
    let filled = host
        .eval_snippet(r##"await fillInput("#q", "hello rust")"##)
        .await
        .expect("fillInput");
    assert_eq!(filled, json!("hello rust"), "fillInput 返回回读值");

    eprintln!("[e2e] pressKey 开始");
    // pressKey：追加字符 + Enter
    host.eval_snippet(
        r#"await session.Runtime.evaluate({expression:"document.querySelector('#q').focus()"})"#,
    )
    .await
    .expect("focus");
    host.eval_snippet("await pressKey(\"!\")")
        .await
        .expect("pressKey");
    let val = host
        .eval_snippet(r#"return (await session.Runtime.evaluate({expression:"document.querySelector('#q').value", returnByValue:true})).result.value"#)
        .await
        .expect("回读");
    assert_eq!(val, json!("hello rust!"), "pressKey 追加: {val}");

    eprintln!("[e2e] clickAt 开始");
    // clickAt：按钮点击置 window.clicked
    host.eval_snippet(
        r#"await session.Page.navigate({url:"data:text/html,<button onclick=\"window.clicked = 7\">Hit</button>"})"#,
    )
    .await
    .expect("导航点击页");
    let center = host
        .eval_snippet(
            r#"return (await session.Runtime.evaluate({expression:"(() => { const r = document.querySelector('button').getBoundingClientRect(); return JSON.stringify([r.x + r.width/2, r.y + r.height/2]) })()", returnByValue:true})).result.value"#,
        )
        .await
        .expect("量中心");
    let (cx, cy) = serde_json::from_str::<(i64, i64)>(center.as_str().unwrap_or("[10,10]"))
        .unwrap_or((10, 10));
    host.eval_snippet(&format!("await clickAt({cx}, {cy})"))
        .await
        .expect("clickAt");
    let clicked = host
        .eval_snippet(r#"await session.waitJs("window.clicked", 3000)"#)
        .await
        .expect("点击生效");
    assert_eq!(clicked, json!(7));

    eprintln!("[e2e] waitLoad 开始");
    // waitLoad：已加载页面立即返回 complete
    let wl = host
        .eval_snippet("return await waitLoad(8000)")
        .await
        .expect("waitLoad");
    assert_eq!(wl.pointer("/readyState"), Some(&json!("complete")), "{wl}");

    eprintln!("[e2e] waitIdle 开始");
    // waitIdle：无网络请求的页面静默即返回
    let wi = host
        .eval_snippet("return await waitIdle(5000)")
        .await
        .expect("waitIdle");
    assert!(wi.get("requests").is_some(), "{wi}");

    eprintln!("[e2e] closeTab 开始");
    // closeTab：关当前活动 tab（newTab 建的 t2，自建 -> 守卫放行）
    let closed = host
        .eval_snippet("return await closeTab((await currentTab()).targetId)")
        .await
        .expect("closeTab 自建");
    assert_eq!(closed, json!(true));
    // chrome 启动自开的初始 tab 不是本会话自建：关它必须被守卫拦
    // （挑 own=false 的，别用 [0]；刚关掉的 t2 可能还在列表缓存里）
    let foreign = host
        .eval_snippet("return await listPageTargets()")
        .await
        .expect("列表2");
    let foreign_id = foreign
        .as_array()
        .and_then(|a| {
            a.iter()
                .find(|t| t.get("own").and_then(Value::as_bool) == Some(false))
        })
        .and_then(|t| t.get("targetId"))
        .and_then(Value::as_str)
        .expect("应存在非自建 tab")
        .to_string();
    let guarded = host
        .eval_snippet(&format!(r#"await closeTab("{foreign_id}")"#))
        .await;
    assert!(
        guarded.is_err() && format!("{guarded:#?}").contains("守卫拦截"),
        "关非自建 tab 应被守卫拦截: {guarded:?}"
    );

    // 守卫：Browser.close 必须被拦（程序级强制，与引擎来源无关）
    let blocked = host.eval_snippet("await session.Browser.close()").await;
    assert!(blocked.is_err(), "Browser.close 应被守卫拦截");

    engine.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn spawn_port_channel_roundtrip() {
    if !gated() {
        eprintln!("skip: BROWSE_E2E 未设 1");
        return;
    }
    let _seq = SEQ.lock().await;
    clean_profile().await;
    let session = cdp::Session::new();
    let host = JsHost::new(session.clone());
    let engine = Engine::new(session);
    engine
        .ensure(&EngineSpec::Auto {
            chrome: None,
            headless: true,
            pipe: false,
        })
        .await
        .expect("端口态引擎起不来（BROWSE_CHROME 指到 clean-chrome 的 chrome.exe？）");
    exercise(&engine, &host, "port").await;
    clean_profile().await;
}

#[tokio::test]
async fn spawn_pipe_channel_roundtrip() {
    if !gated() {
        eprintln!("skip: BROWSE_E2E 未设 1");
        return;
    }
    let _seq = SEQ.lock().await;
    clean_profile().await;
    let session = cdp::Session::new();
    let host = JsHost::new(session.clone());
    let engine = Engine::new(session);
    engine
        .ensure(&EngineSpec::Auto {
            chrome: None,
            headless: true,
            pipe: true,
        })
        .await
        .expect("管道态引擎起不来（需 clean-chrome 2026-09-14 后的 47 锚产物）");
    exercise(&engine, &host, "pipe").await;
    clean_profile().await;
}
