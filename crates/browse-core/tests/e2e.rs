//! 真 chrome 端到端（本机 clean-chrome）。门控：`BROWSE_E2E=1`，CI 无浏览器跳过。
//!
//! 覆盖：spawn headless 引擎（端口态与管道态各一）-> 方言 navigate data: URL
//! -> Runtime.evaluate 读 title -> down 只杀自起实例。

use browse_core::{Engine, EngineSpec, JsHost};
use serde_json::json;

/// 串行化两通道测试：engine-profile 是独占资源（chrome 单实例锁）。
static SEQ: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn gated() -> bool {
    std::env::var("BROWSE_E2E").ok().as_deref() == Some("1")
}

async fn clean_profile() {
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
