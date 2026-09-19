//! 真 chrome 端到端（本机 clean-chrome）。门控：`BROWSE_E2E=1`，CI 无浏览器跳过。
//!
//! 覆盖：spawn headless 引擎（端口态与管道态各一）-> 方言 navigate data: URL
//! -> Runtime.evaluate 读 title 与 `navigator.webdriver` 恒 false -> down 只杀
//! 自起实例。

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

    // navigator.webdriver 恒 false（clean-chrome 补丁族主张，#29 三态验收矩阵）：
    // 本断言随 exercise 跑齐 spawn 的 port 与 pipe 两通道，flat 附着态另测。
    // 判据是锚生效：未打锚的 stock 引擎在此（headless/pipe 均为真值态）应红
    let webdriver = host
        .eval_snippet(
            r#"return (await session.Runtime.evaluate({expression:"navigator.webdriver", returnByValue:true})).result.value"#,
        )
        .await
        .expect("evaluate webdriver");
    assert_eq!(webdriver, json!(false), "navigator.webdriver 应恒 false");

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
    host.eval_snippet(r#"await session.waitFor("Page.frameNavigated", undefined, 15)"#)
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
    host.eval_snippet(r#"await session.waitFor("Page.frameStartedLoading", undefined, 15)"#)
        .await
        .expect("peek 之后 waitFor 仍取得到（非破坏）");

    // waitJs：页内 800ms 后置真，谓词轮询等到
    host.eval_snippet(
        r#"await session.Page.navigate({url:"data:text/html,<script>setTimeout(() => window.done = 42, 800)</script>"})"#,
    )
    .await
    .expect("导航到 waitJs 页");
    let waited = host
        .eval_snippet(r#"await session.waitJs("window.done", 8)"#)
        .await
        .expect("waitJs");
    assert_eq!(waited, json!(42), "waitJs 应返回真值本身");

    // #18 验收：模板字符串 raw 语义经真 V8 往返——内层正则含 \n 与 \d、
    // 页面侧模板 ${} 插值、多行模板（真换行进 expression）
    let tpl_probe = r#"return (await session.Runtime.evaluate({expression: `[/\n/.test("a\nb"), /\d/.test("x7"), \`a${1+1}c\` === "a2c", \`m
l\`.length === 3].join("|")`, returnByValue:true})).result.value"#;
    let via_tpl = host.eval_snippet(tpl_probe).await.expect("模板面 evaluate");
    assert_eq!(via_tpl, json!("true|true|true|true"));
    // A/B：同载荷的旧式手工转义双引号形，两形必须同值（行为等价锁）。
    // 反引号在方言双引号串里本就是普通字符，旧式无需转义它
    let esc_probe = r#"return (await session.Runtime.evaluate({expression: "[/\\n/.test(\"a\\nb\"), /\\d/.test(\"x7\"), `a${1+1}c` === \"a2c\", `m
l`.length === 3].join(\"|\")", returnByValue:true})).result.value"#;
    let via_esc = host.eval_snippet(esc_probe).await.expect("转义面 evaluate");
    assert_eq!(via_esc, via_tpl, "A/B：模板形与手工转义形必须同值");

    // #22 全量 JS 旁路（真 V8）：函数声明/模板字符串/正则直接写零转义，
    // 方言 pageEval 与宿主 eval_js 两入口同值；中文 emoji 往返；大对象
    // returnByValue；页内抛错带 CTA
    // IIFE 包裹：V8 全局词法环境跨求值复用（两次 const 同名会撞），声明收进函数域
    let js = "(() => { function twice(x) { return x * 2; }\nconst s = `n=${twice(21)}`;\nreturn [/\\d/.test(s), s, '中文🚚'].join('|'); })()";
    // 方言侧程序化转义（反斜杠/引号/换行），两入口载荷严格同源
    let esc = js
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    let via_global = host
        .eval_snippet(&format!(r#"return await pageEval("{esc}")"#))
        .await
        .expect("pageEval 真 V8");
    let via_js = host.eval_js(js).await.expect("eval_js 真 V8");
    assert_eq!(via_global, via_js, "两入口同值: {via_global:?}");
    let got = via_js.as_str().unwrap_or_default();
    assert!(
        got.contains("n=42") && got.contains("中文🚚"),
        "模板与中文 emoji 往返: {got:?}"
    );
    // 大对象 returnByValue：百键对象完整回传
    let big = host
        .eval_js("Object.fromEntries(Array.from({length: 100}, (_, i) => [\"k\" + i, i]))")
        .await
        .expect("大对象 returnByValue");
    assert_eq!(big.pointer("/k99"), Some(&json!(99)), "{big:?}");
    // 页内抛错：错误带描述与 CTA（不许裸错误码）
    let threw = host
        .eval_js("throw new Error('e2e-boom')")
        .await
        .expect_err("抛错应上抛");
    let threw_msg = format!("{threw:#}");
    assert!(
        threw_msg.contains("e2e-boom") && threw_msg.contains("下一步"),
        "抛错应带描述与 CTA: {threw_msg}"
    );

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
    // #30 消注入痕：快照链路对主世界零写入，代际标记不存在
    let marker = host
        .eval_snippet(
            r#"return (await session.Runtime.evaluate({expression:"typeof window.__browse_ref_gen", returnByValue:true})).result.value"#,
        )
        .await
        .expect("读注入标记");
    assert_eq!(marker, json!("undefined"), "快照不得向页面写入标记");
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
        .eval_snippet(r#"await session.waitJs("window.go", 3)"#)
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

    // #34 回归锁：同片段 navigate 后紧接 screenshot（提交窗口竞速，
    // 有界重试后必须可靠，修前 data:/https 均 3/3 复现）
    let raced = host
        .eval_snippet(
            r#"await session.Page.navigate({url:"data:text/html,<title>shot</title><h1>ok</h1>"}); return await screenshot()"#,
        )
        .await
        .expect("同片段 navigate+screenshot");
    assert!(
        raced.get("bytes").and_then(Value::as_u64).unwrap_or(0) > 0,
        "竞速窗内截图应有内容: {raced}"
    );
    if let Some(p) = raced.get("path").and_then(Value::as_str) {
        tokio::fs::remove_file(p).await.ok();
    }
    // 提交窗元信息一致性（评审 G2 可观测化）：同片段 navigate+snapshot
    // 的 title 必须与新文档的 document.title 一致（若偶发旧值即红，
    // 根因级方案顺势提前）
    let meta = host
        .eval_snippet(
            r#"await session.Page.navigate({url:"data:text/html,<title>meta3</title><h1>m</h1>"}); const s = await snapshot(); const t = (await session.Runtime.evaluate({expression:"document.title", returnByValue:true})).result.value; return {"snapTitle": s.title, "pageTitle": t}"#,
        )
        .await
        .expect("同片段 navigate+snapshot");
    assert_eq!(
        meta.pointer("/snapTitle"),
        meta.pointer("/pageTitle"),
        "快照 title 应与新文档页内 title 一致: {meta}"
    );

    // ---- 批 1（REQ-006）：goto/历史导航/reload/秒口径 ----
    // goto：navigate+waitLoad 一体，返回提交后世界的 url/title
    let g = host
        .eval_snippet(r#"return await goto("data:text/html,<title>goto-a</title><h1>a</h1>")"#)
        .await
        .expect("goto a");
    assert_eq!(
        g.get("title"),
        Some(&json!("goto-a")),
        "goto 应到 a 页: {g}"
    );
    host.eval_snippet(r#"await goto("data:text/html,<title>goto-b</title><h1>b</h1>")"#)
        .await
        .expect("goto b");
    // goBack：回一步应回到 a 页
    let back = host
        .eval_snippet("return await goBack()")
        .await
        .expect("goBack");
    assert_eq!(
        back.get("title"),
        Some(&json!("goto-a")),
        "goBack 应回 a 页: {back}"
    );
    // goForward：前进回 b 页
    let fwd = host
        .eval_snippet("return await goForward()")
        .await
        .expect("goForward");
    assert_eq!(
        fwd.get("title"),
        Some(&json!("goto-b")),
        "goForward 应回 b 页: {fwd}"
    );
    // reload：加载收尾后 title 不变
    let rl = host
        .eval_snippet("return await reload()")
        .await
        .expect("reload");
    assert_eq!(
        rl.get("title"),
        Some(&json!("goto-b")),
        "reload 后 title 应保持: {rl}"
    );
    // 秒口径（#51）：直写秒与旧毫秒习惯值等价，混用守卫带 timeoutWarning
    let wl_s = host
        .eval_snippet("return await waitLoad(2)")
        .await
        .expect("waitLoad 秒口径");
    assert_eq!(
        wl_s.get("readyState"),
        Some(&json!("complete")),
        "waitLoad(2) 应已加载立即返回: {wl_s}"
    );
    assert!(
        wl_s.get("timeoutWarning").is_none(),
        "直写秒不应告警: {wl_s}"
    );
    let wl_ms = host
        .eval_snippet("return await waitLoad(15000)")
        .await
        .expect("waitLoad 毫秒误写");
    assert!(
        wl_ms
            .get("timeoutWarning")
            .is_some_and(|w| w.as_str().unwrap_or("").contains("毫秒误写")),
        "旧毫秒习惯值应带混用告警: {wl_ms}"
    );
    // clickRef waitNav（#19，评审 F2 补断言）：链接型点击后回执是对象形，
    // waitLoad.readyState 必须可见（修前布尔面附不上键、特性端到端不可见）。
    // 链接目标走 routeMock 的 http 假域——Chrome 禁止顶级跳转 data: URL
    // （点击被拦停在源页，(c) 的 title 断言当场实锤过这个测试设计坑）
    host.eval_snippet(
        r#"await routeMock("http://waitnav.test/*", "<title>waitnav-target</title><h1>t</h1>", {contentType: "text/html"}); await goto("data:text/html,<a href='http://waitnav.test/t'>go</a>")"#,
    )
    .await
    .expect("goto 链接页");
    let lsn = host
        .eval_snippet("return await snapshot()")
        .await
        .expect("链接页 snapshot");
    let link_ref = lsn
        .get("nodes")
        .and_then(Value::as_array)
        .expect("nodes")
        .iter()
        .find(|n| n.get("role") == Some(&json!("link")))
        .and_then(|n| n.get("ref"))
        .and_then(Value::as_str)
        .expect("链接节点应带 ref")
        .to_string();
    let wnav = host
        .eval_snippet(&format!(
            r#"return await clickRef("{link_ref}", {{waitNav: true}})"#
        ))
        .await
        .expect("clickRef waitNav");
    assert_eq!(
        wnav.pointer("/waitLoad/readyState"),
        Some(&json!("complete")),
        "waitNav 回执应带 waitLoad 且已稳定: {wnav}"
    );
    assert_eq!(
        wnav.get("clicked"),
        Some(&json!(true)),
        "clicked 基座在: {wnav}"
    );
    // 评审二轮 F4(c)：readyState 旧文档同样满足，压不出早返——必须再读
    // 导航后文档标记（title 变成目标页才证明真等了新文档）
    let wnav_title = host
        .eval_snippet(
            r#"return (await session.Runtime.evaluate({expression:"document.title", returnByValue:true})).result.value"#,
        )
        .await
        .expect("读 waitNav 后 title");
    assert_eq!(
        wnav_title,
        json!("waitnav-target"),
        "waitNav 后应在新文档（早返旧文档即红）: {wnav_title}"
    );

    // checkRef/uncheckRef：防呆幂等（#39）
    host.eval_snippet(r#"await goto("data:text/html,<input type='checkbox' id='c'>")"#)
        .await
        .expect("goto checkbox 页");
    let csn = host
        .eval_snippet("return await snapshot()")
        .await
        .expect("checkbox snapshot");
    let cb_ref = csn
        .get("nodes")
        .and_then(Value::as_array)
        .expect("nodes")
        .iter()
        .find(|n| n.get("role") == Some(&json!("checkbox")))
        .and_then(|n| n.get("ref"))
        .and_then(Value::as_str)
        .expect("checkbox 节点应带 ref")
        .to_string();
    let ck = host
        .eval_snippet(&format!(r#"return await checkRef("{cb_ref}")"#))
        .await
        .expect("checkRef");
    assert_eq!(
        ck.get("checked"),
        Some(&json!(true)),
        "checkRef 后必 true: {ck}"
    );
    let ck2 = host
        .eval_snippet(&format!(r#"return await checkRef("{cb_ref}")"#))
        .await
        .expect("checkRef 幂等二连");
    assert_eq!(
        ck2.get("clicked"),
        Some(&json!(false)),
        "已勾选再 checkRef 不点击: {ck2}"
    );
    let unck = host
        .eval_snippet(&format!(r#"return await uncheckRef("{cb_ref}")"#))
        .await
        .expect("uncheckRef");
    assert_eq!(
        unck.get("checked"),
        Some(&json!(false)),
        "uncheckRef 后必 false: {unck}"
    );
    // fill submit（#39）：填完顺带 Enter，页内 keydown 副作用可观察
    host.eval_snippet(
        r#"await goto("data:text/html,<input id='q'><script>document.getElementById('q').addEventListener('keydown', e => { if (e.key === 'Enter') document.title = 'submitted' })</script>")"#,
    )
    .await
    .expect("goto 输入页");
    let fs = host
        .eval_snippet(r##"await fillInput("#q", "hi", {submit: true})"##)
        .await
        .expect("fillInput submit");
    assert_eq!(fs, json!("hi"), "fill 返回保持回读串契约: {fs}");
    let t = host
        .eval_snippet(
            r#"return (await session.Runtime.evaluate({expression:"document.title", returnByValue:true})).result.value"#,
        )
        .await
        .expect("读 title");
    assert_eq!(t, json!("submitted"), "Enter 应触发页内提交副作用: {t}");

    // ---- 批 2（REQ-007）：#36 限深与 findRefs、#37 可观测三件、#49 detect ----
    // #36 depth：限深节点数必少于全量，且 childIds 在卷
    host.eval_snippet(
        r#"await goto("data:text/html,<div><h1>T1</h1><p>para</p></div><div><h1>T2</h1><button id=b2 onclick=\"window.frHit=7\">Deep</button></div>")"#,
    )
    .await
    .expect("goto 结构页");
    let full_snap = host
        .eval_snippet("return await snapshot()")
        .await
        .expect("全量 snapshot");
    let full_n = full_snap
        .get("nodes")
        .and_then(Value::as_array)
        .map(|a| a.len())
        .unwrap_or(0);
    let deep_snap = host
        .eval_snippet("return await snapshot({depth: 1})")
        .await
        .expect("限深 snapshot");
    let deep_n = deep_snap
        .get("nodes")
        .and_then(Value::as_array)
        .map(|a| a.len())
        .unwrap_or(0);
    assert!(
        full_n > deep_n && deep_n >= 1,
        "限深应更少: full={full_n} depth1={deep_n}"
    );
    // #36 findRefs：命中只回子集带 ref，且 ref 可直接驱动（fillRef 通道即证）
    let fr = host
        .eval_snippet(r#"return await findRefs("Deep", {context: 1})"#)
        .await
        .expect("findRefs");
    // 按钮与其 StaticText 子文本都含 Deep，count=2（name 子串匹配面如实）
    assert_eq!(
        fr.get("count"),
        Some(&json!(2)),
        "应命中按钮与文本两节点: {fr}"
    );
    let fr_nodes = fr
        .get("nodes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert!(
        fr_nodes.len() < full_n,
        "命中加祖先链应远少于全量: {}",
        fr_nodes.len()
    );
    let hit_ref = fr_nodes
        .iter()
        .find(|n| n.get("hit") == Some(&json!(true)) && n.get("role") == Some(&json!("button")))
        .and_then(|n| n.get("ref"))
        .and_then(Value::as_str)
        .expect("命中节点应带 ref")
        .to_string();
    // 命中 ref 直接可驱动（引用表已替换为 findRefs 结果集）
    host.eval_snippet(&format!(r#"await clickRef("{hit_ref}")"#))
        .await
        .expect("clickRef 命中 ref");
    let fr_hit = host
        .eval_snippet(r#"return (await session.Runtime.evaluate({expression:"window.frHit", returnByValue:true})).result.value"#)
        .await
        .expect("读 frHit");
    assert_eq!(fr_hit, json!(7), "findRefs 的 ref 应可直接点击: {fr_hit}");
    // #37 console 分级：log 不进 error 档，error 进
    // 抛错走 setTimeout（同步 throw 会被 pageEval 当调用失败返回，
    // 异步抛才落 Runtime.exceptionThrown）；哨兵等它落地
    host.eval_snippet(
        r#"await pageEval("console.log('c-log'); console.error('c-err'); setTimeout(() => { throw new Error('uncaught-boom') }, 0); setTimeout(() => { window.__t = 1 }, 50)")"#,
    )
    .await
    .expect("页内 console 与未捕获异常");
    host.eval_snippet(r#"await session.waitJs("window.__t === 1", 3)"#)
        .await
        .expect("等哨兵");
    let cerr = host
        .eval_snippet(r#"return await console({minLevel: "error"})"#)
        .await
        .expect("console error 档");
    let msgs = cerr
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert!(
        msgs.iter().any(|m| m.get("text") == Some(&json!("c-err"))),
        "error 档应含 c-err: {cerr}"
    );
    assert!(
        !msgs.iter().any(|m| m.get("text") == Some(&json!("c-log"))),
        "error 档不应含 c-log: {cerr}"
    );
    // #37 jsErrors：未捕获异常可取
    let je = host
        .eval_snippet("return await jsErrors()")
        .await
        .expect("jsErrors");
    let errs = je
        .get("errors")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert!(
        errs.iter().any(|e| e
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .contains("uncaught-boom")),
        "jsErrors 应含 uncaught-boom: {je}"
    );
    // #37 requests/detail：routeMock 假域出对账（status 200 带 requestId）
    host.eval_snippet(
        r#"await routeMock("http://obs.test/api*", "{}", {contentType: "application/json"}); await pageEval("fetch('http://obs.test/api1').then(r => r.text())")"#,
    )
    .await
    .expect("mock 与 fetch");
    host.eval_snippet(r#"await session.waitJs("window.__ok || true", 1)"#)
        .await
        .ok();
    let rq = host
        .eval_snippet(r#"return await requests({filter: "obs.test"})"#)
        .await
        .expect("requests");
    let rows = rq
        .get("requests")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert!(!rows.is_empty(), "requests 应有 obs.test 条目: {rq}");
    let rid = rows[0]
        .get("requestId")
        .and_then(Value::as_str)
        .expect("requestId")
        .to_string();
    let rd = host
        .eval_snippet(&format!(r#"return await requestDetail("{rid}")"#))
        .await
        .expect("requestDetail");
    assert_eq!(rd.get("status"), Some(&json!(200)), "detail 应 200: {rd}");
    // #49 detect：正常页 ok、空白页 blank、密码页 login-wall
    host.eval_snippet(
        r#"await goto("data:text/html,<title>d-ok</title><h1>full</h1><p>words words words words words</p>")"#,
    )
    .await
    .expect("goto 正常页");
    let det = host
        .eval_snippet("return await detect()")
        .await
        .expect("detect ok");
    assert_eq!(det.get("verdict"), Some(&json!("ok")), "正常页应 ok: {det}");
    assert!(
        det.get("evidence")
            .and_then(Value::as_array)
            .is_some_and(|e| !e.is_empty()),
        "ok 判证据应非空: {det}"
    );
    host.eval_snippet(r#"await goto("data:text/html,<title>d-blank</title>")"#)
        .await
        .expect("goto 空白页");
    let detb = host
        .eval_snippet("return await detect()")
        .await
        .expect("detect blank");
    assert_eq!(
        detb.get("verdict"),
        Some(&json!("blank")),
        "空白页应 blank: {detb}"
    );
    host.eval_snippet(
        r#"await goto("data:text/html,<title>d-login</title><form><input type=password></form>")"#,
    )
    .await
    .expect("goto 登录页");
    let detl = host
        .eval_snippet("return await detect()")
        .await
        .expect("detect login");
    assert_eq!(
        detl.get("verdict"),
        Some(&json!("login-wall")),
        "密码页应 login-wall: {detl}"
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
    // 先开 Network 域再触发请求：响应事件只在域已开时投递（waitForResponse
    // 的幂等开域在未开时会错过已完成的响应）
    host.eval_snippet("await session.Network.enable({})")
        .await
        .expect("Network.enable");
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
        .eval_snippet(r#"await session.waitJs("window.got", 8)"#)
        .await
        .expect("mock 应答应到达");
    assert_eq!(got, json!("{\"ok\":1}"), "mock body 应原样到达: {got}");

    // #20 验收：waitForResponse 取最近命中（mock 应答已在缓冲），四件齐
    let wr = host
        .eval_snippet(r#"return await waitForResponse("http://mock.test/api*")"#)
        .await
        .expect("waitForResponse");
    assert_eq!(wr.pointer("/status"), Some(&json!(200)), "{wr}");
    assert_eq!(wr.pointer("/body"), Some(&json!("{\"ok\":1}")), "{wr}");
    assert_eq!(wr.pointer("/json/ok"), Some(&json!(1)), "{wr}");
    assert!(
        wr.pointer("/url")
            .and_then(Value::as_str)
            .is_some_and(|u| u.contains("mock.test/api")),
        "{wr}"
    );
    // #20 超时路径：不命中的 pattern 短窗报可读错误
    let miss = host
        .eval_snippet(r#"await waitForResponse("http://never.test/*", 1)"#)
        .await;
    let miss_msg = format!("{miss:#?}");
    assert!(
        miss.is_err() && miss_msg.contains("超时") && miss_msg.contains("下一步"),
        "未命中应报超时加 CTA: {miss_msg}"
    );
    assert!(
        miss_msg.contains("去重"),
        "CTA 清单应封顶去重（F3）: {miss_msg}"
    );
    // #20 A/B：便捷函数与手工路线（findEvents 加 responseBody）取同值
    let manual = host
        .eval_snippet(
            r#"const evs = await session.findEvents("Network.responseReceived", "params.response.url", "http://mock.test/api/data", 1)
return await responseBody(evs[0].params.requestId)"#,
        )
        .await
        .expect("手工路线");
    assert_eq!(
        manual.pointer("/body"),
        wr.pointer("/body"),
        "A/B：两路 body 必须同值: manual={manual} wr={wr}"
    );
    // #20 慢体路径的体就绪等待与 bodyError 显式化在 js_host 单测锁
    // （connect_pipes 假 CDP 对端，确定性时序）；真网慢体依赖 WSL 到
    // Windows 引擎的回环边界，此处不重复
    // #19 验收：use 换靶同步开 Page 域——不显式 enable，立即导航后增量
    // 游标等到新事件（旧路径 300ms 轮询开域会丢换靶后的首批事件）
    let ct = host
        .eval_snippet("return await currentTab()")
        .await
        .expect("currentTab");
    let tid = ct
        .get("targetId")
        .and_then(Value::as_str)
        .expect("targetId")
        .to_string();
    let cur = host
        .eval_snippet(r#"return await session.peekEvents("Page.frameStartedLoading", 1)"#)
        .await
        .expect("游标取样");
    let cursor = cur
        .as_array()
        .and_then(|a| a.last())
        .and_then(|e| e.get("seq"))
        .and_then(Value::as_u64)
        .expect("seq");
    host.eval_snippet(&format!(r#"await session.use("{tid}")"#))
        .await
        .expect("use 换靶");
    host.eval_snippet(r#"await session.Page.navigate({url:"data:text/html,<title>r19</title>"})"#)
        .await
        .expect("导航 r19");
    let since = host
        .eval_snippet(&format!(
            r#"return await session.peekEventsSince("Page.frameStartedLoading", {cursor}, 3)"#
        ))
        .await
        .expect("增量等事件");
    assert!(
        since.as_array().is_some_and(|a| !a.is_empty()),
        "#19 换靶后应立即有事件: {since}"
    );
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
        .eval_snippet(r#"await session.waitJs("window.err", 8)"#)
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
        r#"await session.waitJs("document.body && document.body.innerText.includes('one')", 5)"#,
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

    // #33 录制 × 换靶（pin 保护）：录制中开新 tab 换靶，录制 session 被
    // 钉住不 detach，帧流存活；recordStop 定向停本体，回执带 sessionChanged
    let rec_cur = host
        .eval_snippet("return await currentTab()")
        .await
        .expect("录制前当前 tab");
    let rec_tid = rec_cur
        .get("targetId")
        .and_then(Value::as_str)
        .expect("targetId")
        .to_string();
    let rec2 = host
        .eval_snippet("return await recordStart()")
        .await
        .expect("recordStart 2");
    assert!(rec2.get("dir").is_some(), "recordStart 回 dir: {rec2}");
    host.eval_snippet(r#"await session.Page.navigate({url:"data:text/html,<h3>rec-two</h3>"})"#)
        .await
        .expect("录制中导航 3");
    host.eval_snippet(r#"await session.waitJs("document.body && document.body.innerText.includes('rec-two')", 5)"#)
        .await
        .expect("等 rec-two");
    host.eval_snippet("await newTab()")
        .await
        .expect("录制中开新 tab（换靶）");
    let stopped2 = host
        .eval_snippet("return await recordStop()")
        .await
        .expect("recordStop 2");
    assert_eq!(
        stopped2.pointer("/sessionChanged"),
        Some(&json!(true)),
        "换靶后停应告警: {stopped2}"
    );
    let frames2 = stopped2.get("frames").and_then(Value::as_u64).unwrap_or(0);
    assert!(frames2 >= 1, "pin 下帧流应跨换靶存活: {stopped2}");
    let rec_dir2 = stopped2
        .get("dir")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let _ = tokio::fs::remove_dir_all(&rec_dir2).await;
    // 清场：关自开 tab，切回录制 tab
    let nt = host
        .eval_snippet("return await currentTab()")
        .await
        .expect("当前新 tab");
    let nt_id = nt
        .get("targetId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if !nt_id.is_empty() {
        host.eval_snippet(&format!(r#"await closeTab("{nt_id}")"#))
            .await
            .expect("关自开 tab");
    }
    host.eval_snippet(&format!(r#"await switchTab("{rec_tid}")"#))
        .await
        .expect("切回录制 tab");
    // 没在录时 recordStop：错误带 CTA
    let no_rec = host.eval_snippet("await recordStop()").await;
    assert!(
        no_rec.is_err() && format!("{no_rec:#?}").contains("recordStart"),
        "空停应带开录 CTA: {no_rec:?}"
    );

    // 评审 F1 跨 tab 泄漏锁：后台 tab 的 403 与 console.error 不劫持
    // 活动 tab 的 detect 判读与 console 检索（修前干净页被判 blocked）
    host.eval_snippet(
        r#"await goto("data:text/html,<title>clean-a</title><h1>clean page with enough body text</h1>")"#,
    )
    .await
    .expect("tab A 干净页");
    // goto 回执不带 targetId，活动 tab id 走 currentTab()
    let tab_a = host
        .eval_snippet("return await currentTab()")
        .await
        .expect("A currentTab");
    let a_id = tab_a
        .get("targetId")
        .and_then(Value::as_str)
        .map(str::to_string);
    host.eval_snippet(
        r#"await newTab("data:text/html,<title>tab-b</title><script>console.error('from-tab-B'); fetch('http://b403.test/x')</script>")"#,
    )
    .await
    .expect("tab B");
    host.eval_snippet(
        r#"await routeMock("http://b403.test/*", "{}", {status: 403}); await pageEval("fetch('http://b403.test/x').catch(() => 0)")"#,
    )
    .await
    .expect("B 打 403");
    host.eval_snippet(r#"await session.waitJs("true", 1)"#)
        .await
        .ok();
    // 切回 A：detect 必须 ok、console error 档不见 B 的错
    if let Some(a) = &a_id {
        host.eval_snippet(&format!(r#"await switchTab("{a}")"#))
            .await
            .expect("切回 A");
    }
    let det_a = host
        .eval_snippet("return await detect()")
        .await
        .expect("A detect");
    assert_eq!(
        det_a.get("verdict"),
        Some(&json!("ok")),
        "后台 tab 的 403 不得劫持活动页判读: {det_a}"
    );
    let con_a = host
        .eval_snippet(r#"return await console({minLevel: "error"})"#)
        .await
        .expect("A console");
    let con_msgs = con_a
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert!(
        !con_msgs
            .iter()
            .any(|m| m.get("text") == Some(&json!("from-tab-B"))),
        "他 tab 的 console.error 不得泄漏: {con_a}"
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

    // #33 F1 回归锁：多次换靶后（use/newTab/switchTab 全走过）陈旧
    // session 已 detach，一次导航的响应事件在缓冲里只此一份
    host.eval_snippet("await session.Network.enable({})")
        .await
        .expect("Network.enable t2");
    host.eval_snippet(
        r#"await session.Page.navigate({url:"data:text/html,<title>dedup</title>"})"#,
    )
    .await
    .expect("导航 dedup");
    let dup = host
        .eval_snippet(r#"return await session.peekEvents("Network.responseReceived", 50)"#)
        .await
        .expect("peek dedup");
    let dedup_events: Vec<&Value> = dup
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|e| {
                    e.pointer("/params/response/url")
                        .and_then(Value::as_str)
                        .is_some_and(|u| u.contains("dedup"))
                })
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(
        dedup_events.len(),
        1,
        "换靶后同一响应应只投递一份（F1 detach）: {dup}"
    );
    // 探针页还原：把 t2 导回 tab-two 原页（后续 fillInput 要 #q）
    host.eval_snippet(r#"await session.Page.navigate({url:"data:text/html,<title>tab-two</title><input id=\"q\" value=\"old\">"})"#)
        .await
        .expect("还原 t2 页");

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
        .eval_snippet(r#"await session.waitJs("window.clicked", 3)"#)
        .await
        .expect("点击生效");
    assert_eq!(clicked, json!(7));

    eprintln!("[e2e] waitLoad 开始");
    // waitLoad：已加载页面立即返回 complete
    let wl = host
        .eval_snippet("return await waitLoad(8)")
        .await
        .expect("waitLoad");
    assert_eq!(wl.pointer("/readyState"), Some(&json!("complete")), "{wl}");

    eprintln!("[e2e] waitIdle 开始");
    // waitIdle：无网络请求的页面静默即返回
    let wi = host
        .eval_snippet("return await waitIdle(5)")
        .await
        .expect("waitIdle");
    assert!(wi.get("requests").is_some(), "{wi}");

    eprintln!("[e2e] 薄封装批（#23/#24/#25.2/#25.3）开始");
    // hover（#23）：:hover 触发子菜单显形
    host.eval_snippet(
        r#"await session.Page.navigate({url:"data:text/html,<style>.m{display:none}.t:hover .m{display:block}</style><button class=t>Ho<span class=m>shown</span></button>"})"#,
    )
    .await
    .expect("导航 hover 页");
    let hsn = host
        .eval_snippet("return await snapshot()")
        .await
        .expect("hover snapshot");
    let hover_ref = hsn
        .get("nodes")
        .and_then(Value::as_array)
        .expect("nodes")
        .iter()
        .find(|n| n.get("name") == Some(&json!("Ho")))
        .and_then(|n| n.get("ref"))
        .and_then(Value::as_str)
        .expect("hover 目标 ref")
        .to_string();
    host.eval_snippet(&format!(r#"await hoverRef("{hover_ref}")"#))
        .await
        .expect("hoverRef");
    let shown = host
        .eval_snippet(r#"await session.waitJs("getComputedStyle(document.querySelector('.m')).display === 'block'", 3)"#)
        .await;
    assert!(shown.is_ok(), "hover 应触发 :hover 显形: {shown:?}");
    // dblclick（#23）：计数器到 2
    host.eval_snippet(
        r#"await session.Page.navigate({url:"data:text/html,<button id=d onclick='window.n=(window.n||0)+1' ondblclick='window.db=(window.db||0)+1'>Dbl</button>"})"#,
    )
    .await
    .expect("导航 dblclick 页");
    let dsn = host
        .eval_snippet("return await snapshot()")
        .await
        .expect("dbl snapshot");
    let dbl_ref = dsn
        .get("nodes")
        .and_then(Value::as_array)
        .expect("nodes")
        .iter()
        .find(|n| n.get("name") == Some(&json!("Dbl")))
        .and_then(|n| n.get("ref"))
        .and_then(Value::as_str)
        .expect("dbl ref")
        .to_string();
    host.eval_snippet(&format!(r#"await dblclickRef("{dbl_ref}")"#))
        .await
        .expect("dblclickRef");
    let db = host
        .eval_snippet(r#"await session.waitJs("window.db", 3)"#)
        .await
        .expect("dblclick 应触发 ondblclick");
    assert_eq!(db, json!(1));
    // pressKey 组合（#23）：Control+a 全选后覆盖输入
    host.eval_snippet(
        r#"await session.Page.navigate({url:"data:text/html,<input id=q value='old'>"})"#,
    )
    .await
    .expect("导航组合键页");
    host.eval_snippet(r##"await fillInput("#q", "first")"##)
        .await
        .expect("fillInput first");
    host.eval_snippet(r#"await clickAt(10, 10)"#)
        .await
        .expect("点页面");
    let _ = host.eval_snippet(r#"await session.Runtime.evaluate({expression:"document.getElementById('q').focus()"})"#).await;
    host.eval_snippet(r#"await pressKey("Control+a")"#)
        .await
        .expect("pressKey 组合");
    host.eval_snippet(r#"await session.Runtime.evaluate({expression:"document.execCommand('insertText', false, 'Z')"})"#)
        .await
        .expect("选中态输入 Z");
    let qv = host
        .eval_snippet(r#"return (await session.Runtime.evaluate({expression:"document.getElementById('q').value", returnByValue:true})).result.value"#)
        .await
        .expect("回读 q");
    assert_eq!(qv, json!("Z"), "Control+a 应全选后覆盖: {qv}");
    // typeRef（#23）：contenteditable 真实按键序列
    host.eval_snippet(
        r#"await session.Page.navigate({url:"data:text/html,<div id=ce contenteditable role=textbox aria-label='CE' tabindex=0>CE</div>"})"#,
    )
    .await
    .expect("导航 typeRef 页");
    let tsn = host
        .eval_snippet("return await snapshot()")
        .await
        .expect("type snapshot");
    let ce_ref = tsn
        .get("nodes")
        .and_then(Value::as_array)
        .expect("nodes")
        .iter()
        .find(|n| n.get("name") == Some(&json!("CE")))
        .and_then(|n| n.get("ref"))
        .and_then(Value::as_str)
        .expect("ce ref")
        .to_string();
    host.eval_snippet(&format!(r#"await typeRef("{ce_ref}", "hi")"#))
        .await
        .expect("typeRef");
    let cev = host
        .eval_snippet(
            r#"await session.waitJs("document.getElementById('ce').innerText.includes('hi')", 3)"#,
        )
        .await;
    assert!(cev.is_ok(), "typeRef 应入 contenteditable: {cev:?}");
    // initScript（#25.2）：设脚本后新文档生效；替换不叠加、清除真撤
    //（评审 F1 回归锁：identifier 记账，removeScript 先撤再 add）
    host.eval_snippet(r#"await setInitScript("window.__init_probe = 41")"#)
        .await
        .expect("setInitScript");
    host.eval_snippet(r#"await session.Page.navigate({url:"data:text/html,<title>ini</title>"})"#)
        .await
        .expect("导航 init 页");
    let ini = host
        .eval_snippet(r#"await session.waitJs("window.__init_probe === 41", 3)"#)
        .await;
    assert!(ini.is_ok(), "init 脚本应在新文档前置执行: {ini:?}");
    host.eval_snippet(r#"await setInitScript("window.__init_probe2 = 42")"#)
        .await
        .expect("setInitScript 替换");
    host.eval_snippet(r#"await session.Page.navigate({url:"data:text/html,<title>ini2</title>"})"#)
        .await
        .expect("导航 init 页 2");
    let ini2 = host
        .eval_snippet(r#"await session.waitJs("window.__init_probe2 === 42", 3)"#)
        .await;
    assert!(ini2.is_ok(), "新脚本应生效: {ini2:?}");
    let old_gone = host
        .eval_snippet(r#"await session.waitJs("window.__init_probe === 41", 1)"#)
        .await;
    assert!(
        old_gone.is_err(),
        "替换后旧脚本不应再跑（不叠加）: {old_gone:?}"
    );
    host.eval_snippet(r#"await setInitScript("")"#)
        .await
        .expect("setInitScript 清除");
    host.eval_snippet(r#"await session.Page.navigate({url:"data:text/html,<title>ini3</title>"})"#)
        .await
        .expect("导航 init 页 3");
    let cleared = host
        .eval_snippet(r#"await session.waitJs("window.__init_probe2 === 42", 1)"#)
        .await;
    assert!(cleared.is_err(), "清除后不应再跑（注册真撤）: {cleared:?}");
    // storageState（#25.3）：cookies 半边往返（CDP setCookies 不经网络）
    host.eval_snippet(r#"await session.Network.setCookies({cookies:[{name:"probe", value:"v1", domain:"mock.test", path:"/"}]})"#)
        .await
        .expect("setCookies");
    host.eval_snippet("const st = await exportStorageState()")
        .await
        .expect("exportStorageState");
    let cookie_n = host
        .eval_snippet("return st.cookies.length")
        .await
        .expect("cookie 计数");
    assert!(
        cookie_n.as_u64().unwrap_or(0) >= 1,
        "导出应含探针 cookie: {cookie_n}"
    );
    let imp = host
        .eval_snippet("return await importStorageState(st)")
        .await
        .expect("importStorageState");
    assert!(
        imp.pointer("/cookies").and_then(Value::as_u64).unwrap_or(0) >= 1,
        "导入应回写 cookie: {imp}"
    );
    // storageState 的 localStorage 半边（评审 G5）：routeMock 造 http origin，
    // 写 -> 导 -> 清 -> 导入 -> 回读同值
    host.eval_snippet(r#"await routeMock("http://ls.test/*", "<html><body>LS</body></html>")"#)
        .await
        .expect("routeMock ls");
    host.eval_snippet(r#"await session.Page.navigate({url:"http://ls.test/x"})"#)
        .await
        .expect("导航 ls 页");
    host.eval_snippet(
        r#"await session.waitJs("document.body && document.body.innerText.includes('LS')", 5)"#,
    )
    .await
    .expect("ls 页就绪");
    host.eval_snippet(
        r#"await session.Runtime.evaluate({expression:"localStorage.setItem('k', 'v1')"})"#,
    )
    .await
    .expect("写 localStorage");
    host.eval_snippet("const st = await exportStorageState()")
        .await
        .expect("导出含 ls");
    let ls_n = host
        .eval_snippet("return st.origins[0].localStorage.length")
        .await
        .expect("ls 计数");
    assert_eq!(ls_n, json!(1), "导出应含一条 localStorage: {ls_n}");
    host.eval_snippet(r#"await session.Runtime.evaluate({expression:"localStorage.clear()"})"#)
        .await
        .expect("清 localStorage");
    let after_clear = host
        .eval_snippet(r#"return (await session.Runtime.evaluate({expression:"localStorage.getItem('k')", returnByValue:true})).result.value"#)
        .await
        .expect("清后回读");
    assert_eq!(after_clear, Value::Null, "清后应为 null: {after_clear}");
    host.eval_snippet("await importStorageState(st)")
        .await
        .expect("导入 ls");
    let after_import = host
        .eval_snippet(r#"return (await session.Runtime.evaluate({expression:"localStorage.getItem('k')", returnByValue:true})).result.value"#)
        .await
        .expect("导入后回读");
    assert_eq!(after_import, json!("v1"), "导入应回同值: {after_import}");
    host.eval_snippet("await routeClear()")
        .await
        .expect("routeClear");

    // screenshot 选项（#24）：ifChanged 第二次 skipped，jpeg 出 .jpg
    //（先清残留：/tmp 跨测试运行持久，同画面会假 skipped）
    let _ = tokio::fs::remove_file("/tmp/browse-e2e-shot.png").await;
    let _ = tokio::fs::remove_file("/tmp/browse-e2e-shot.jpg").await;
    let shot1 = host
        .eval_snippet(
            r#"return await screenshot("/tmp/browse-e2e-shot.png", false, {ifChanged: true})"#,
        )
        .await
        .expect("shot1");
    assert_eq!(shot1.pointer("/skipped"), Some(&json!(false)), "{shot1}");
    let shot2 = host
        .eval_snippet(
            r#"return await screenshot("/tmp/browse-e2e-shot.png", false, {ifChanged: true})"#,
        )
        .await
        .expect("shot2");
    assert_eq!(
        shot2.pointer("/skipped"),
        Some(&json!(true)),
        "同画面应跳过: {shot2}"
    );
    let jpg = host
        .eval_snippet(r#"return await screenshot("/tmp/browse-e2e-shot", false, {format: "jpeg", quality: 70})"#)
        .await
        .expect("jpeg shot");
    assert!(
        jpg.pointer("/path")
            .and_then(Value::as_str)
            .is_some_and(|p| p.ends_with(".jpg")),
        "jpeg 应 .jpg: {jpg}"
    );
    let _ = tokio::fs::remove_file("/tmp/browse-e2e-shot.png").await;
    let _ = tokio::fs::remove_file("/tmp/browse-e2e-shot.jpg").await;
    // emulate（#24）：视口改写生效
    host.eval_snippet(r#"await emulate({viewport:{width:500,height:400}})"#)
        .await
        .expect("emulate");
    let iw = host
        .eval_snippet(r#"return (await session.Runtime.evaluate({expression:"window.innerWidth", returnByValue:true})).result.value"#)
        .await
        .expect("innerWidth");
    assert_eq!(iw, json!(500), "视口应 500: {iw}");
    host.eval_snippet(r#"await session.Emulation.clearDeviceMetricsOverride({})"#)
        .await
        .expect("清仿真");

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
            profile: None,
            proxy: None,
            proxy_bypass: None,
            isolated: false,
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
            profile: None,
            proxy: None,
            proxy_bypass: None,
            isolated: false,
        })
        .await
        .expect("管道态引擎起不来（需 clean-chrome 2026-09-14 后的 47 锚产物）");
    exercise(&engine, &host, "pipe").await;
    clean_profile().await;
}

/// 隔离态 profile（#25.3 拆出，#25.1 线）：isolated 落 state/isolated-*
/// 目录，引擎退场目录即删。
#[tokio::test]
async fn isolated_profile_removed_on_shutdown() {
    if !gated() {
        eprintln!("skip: BROWSE_E2E 未设 1");
        return;
    }
    let _seq = SEQ.lock().await;
    clean_profile().await;
    let session = cdp::Session::new();
    let engine = Engine::new(session);
    engine
        .ensure(&EngineSpec::Auto {
            chrome: None,
            headless: true,
            pipe: false,
            profile: None,
            proxy: None,
            proxy_bypass: None,
            isolated: true,
        })
        .await
        .expect("隔离态引擎");
    let src = engine.source().await;
    let browse_core::EngineSource::Spawned { profile_dir, .. } = &src else {
        panic!("应 Spawned: {src:?}");
    };
    let dir = profile_dir.clone();
    assert!(
        dir.to_string_lossy().contains("isolated-"),
        "隔离目录命名: {dir:?}"
    );
    assert!(dir.is_dir(), "运行中目录应在: {dir:?}");
    engine.shutdown().await.expect("shutdown");
    assert!(!dir.exists(), "退场后目录应删: {dir:?}");
}
