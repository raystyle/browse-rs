//! 临时评审探针（评审后即删）：#43 F1/F2 复核。

use browse_core::{Engine, EngineSpec, JsHost};

fn gated() -> bool {
    std::env::var("BROWSE_E2E").ok().as_deref() == Some("1")
}

#[tokio::test]
async fn record_v1_recheck() {
    if !gated() {
        eprintln!("skip: BROWSE_E2E 未设 1");
        return;
    }
    let session = cdp::Session::new();
    let host = JsHost::new(session.clone());
    let engine = Engine::new(session.clone());
    engine
        .ensure(&EngineSpec::Auto {
            chrome: None,
            headless: true,
            pipe: false,
            profile: None,
            proxy: None,
            proxy_bypass: None,
            isolated: false,
        })
        .await
        .expect("引擎");
    host.eval_snippet(
        r#"await routeMock("http://rec43b.test/*", "<div style='height:3000px'>page</div>", {contentType: "text/html"}); await goto("http://rec43b.test/x", {timeout: 10})"#,
    )
    .await
    .expect("页");
    host.eval_snippet(r#"return await recordStart({cursor: true, showActions: true})"#)
        .await
        .expect("recordStart");
    let read_cur = r#"return (await session.Runtime.evaluate({expression:"(() => { const c = document.getElementById('browse-rec-cursor'); return JSON.stringify({left: c.style.left, top: c.style.top, x: Math.round(c.getBoundingClientRect().x), y: Math.round(c.getBoundingClientRect().y)}) })()", returnByValue:true})).result.value"#;
    eprintln!("[probe] 初始光标 = {:?}", host.eval_snippet(read_cur).await);
    // F1-a：mouseMove 路径
    host.eval_snippet("await mouseMove(300, 200)")
        .await
        .expect("mouseMove");
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    eprintln!(
        "[probe] mouseMove(300,200) 后 = {:?}",
        host.eval_snippet(read_cur).await
    );
    // F1-b：clickAt 路径（不走 mouse_move）
    host.eval_snippet("await clickAt(500, 600)")
        .await
        .expect("clickAt");
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    eprintln!(
        "[probe] clickAt(500,600) 后 = {:?}（不变即只有 mouseMove 路径跟随）",
        host.eval_snippet(read_cur).await
    );
    // F1-c：hoverRef / clickRef 等同理（用 clickRef 走一段：先 snapshot）
    let snp = host
        .eval_snippet("return await snapshot()")
        .await
        .expect("snap");
    let r0 = snp["nodes"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|n| n["ref"].as_str())
        .map(str::to_string);
    if let Some(r) = r0 {
        let _ = host
            .eval_snippet(&format!(r#"return await hoverRef("{r}")"#))
            .await;
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        eprintln!(
            "[probe] hoverRef 后 = {:?}",
            host.eval_snippet(read_cur).await
        );
    }
    // F2-a：监听幂等 + stop 摘除
    let _ = host.eval_snippet(r#"return await recordStop()"#).await; // 先收干净（此前一场还在录）
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    host.eval_snippet(
        r#"await pageEval("window.__c=0; window.__r=0; window.__has=null; const _ae=EventTarget.prototype.addEventListener; EventTarget.prototype.addEventListener=function(t,f,o){ if(t==='click') window.__c++; return _ae.call(this,t,f,o) }; const _re=EventTarget.prototype.removeEventListener; EventTarget.prototype.removeEventListener=function(t,f,o){ if(t==='click') window.__r++; return _re.call(this,t,f,o) }")"#,
    )
    .await
    .expect("钩监听");
    for _ in 0..2 {
        host.eval_snippet(r#"return await recordStop()"#).await.ok();
        host.eval_snippet(r#"return await recordStart({showActions: true})"#)
            .await
            .expect("再开");
    }
    let added = host
        .eval_snippet(
            r#"return (await session.Runtime.evaluate({expression:"JSON.stringify({added: window.__c, removed: window.__r, globalSet: typeof window.__browseShowActions})", returnByValue:true})).result.value"#,
        )
        .await;
    eprintln!(
        "[probe] 两轮 stop/start 后 = {added:?}（added 应 2、removed 应 2、globalSet 应 object）"
    );
    // F2-b：滚动后闪圈落点（pageX/pageY 与光标页面坐标口径是否一致）
    host.eval_snippet(r#"return await recordStop()"#).await.ok();
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    host.eval_snippet(r#"return await recordStart({showActions: true})"#)
        .await
        .expect("开新场");
    let has_handler = host
        .eval_snippet(
            r#"return (await session.Runtime.evaluate({expression:"typeof window.__browseShowActions", returnByValue:true})).result.value"#,
        )
        .await;
    eprintln!("[probe] 新场监听已挂 = {has_handler:?}");
    host.eval_snippet(r#"await pageEval("window.__ring = null; const _oa = Element.prototype.appendChild; Element.prototype.appendChild = function(c){ if (c && c.id !== 'browse-rec-cursor' && (c.getAttribute && (c.getAttribute('style')||'').includes('#ff8c00'))) window.__ring = {left: c.style.left, top: c.style.top}; return _oa.call(this, c) }")"#)
        .await
        .expect("钩 appendChild");
    host.eval_snippet(r#"await pageEval("window.scrollTo(0, 500); void 0")"#)
        .await
        .expect("滚");
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    host.eval_snippet("await clickAt(400, 300)")
        .await
        .expect("点击");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let ring = host
        .eval_snippet(
            r#"return (await session.Runtime.evaluate({expression:"JSON.stringify({ring: window.__ring, scrollY: window.scrollY, expectTop: 500 + 300 - 22, expectLeft: 400 - 22})", returnByValue:true})).result.value"#,
        )
        .await;
    eprintln!("[probe] 滚 500 后点击落点 = {ring:?}（ring.top 应约 778 = scrollY+clientY-22）");
    let _ = host.eval_snippet("return await recordStop()").await;
    engine.shutdown().await.expect("shutdown");
}
