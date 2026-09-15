//! 真 chrome 端到端（本机 clean-chrome）。门控：`BROWSE_E2E=1`，CI 无浏览器跳过。
//!
//! 覆盖：spawn headless 引擎（端口态与管道态各一）-> 方言 navigate data: URL
//! -> Runtime.evaluate 读 title -> down 只杀自起实例。

use browse_core::{Engine, EngineSpec, JsHost};
use serde_json::{Value, json};

/// 串行化两通道测试：engine-profile 是独占资源（chrome 单实例锁）。
static SEQ: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn gated() -> bool {
    std::env::var("BROWSE_E2E").ok().as_deref() == Some("1")
}

async fn clean_profile() {
    // 先收割上次被杀测试留下的引擎 chrome（只认 browse-rs 部署路径，
    // 不碰用户浏览器），否则单实例 profile 锁让本次 spawn 连坐挂死
    let _ = tokio::task::spawn_blocking(|| {
        let _ = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "Get-Process chrome -ErrorAction SilentlyContinue | Where-Object {$_.Path -like '*browse-rs*'} | Stop-Process -Force",
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    })
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    let dir = browse_core::engine::engine_profile_dir();
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
        r#"await session.Page.navigate({url:"data:text/html,<title>ax</title><button>GoGo</button><input value=\"hi\">"})"#,
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
