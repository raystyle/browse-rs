//! 方言错误 CTA 契约：每条错误必须带「下一步：」指引（G002 形态），
//! 语法错误还必须带「行L:列C」位置。未连接 Session 即可触发全部求值层错误，
//! 无需浏览器。

use browse_core::JsHost;
use cdp::Session;

async fn err_of(src: &str) -> String {
    let session = Session::new();
    let host = JsHost::new(session);
    host.eval_snippet(src)
        .await
        .expect_err(&format!("应报错: {src}"))
        .to_string()
}

#[tokio::test]
async fn syntax_errors_carry_location_and_next_step() {
    let bad = [
        ("if (x) { y() }", "Runtime.evaluate"),
        ("const f = (x) => x", "Runtime.evaluate"),
        ("const s = `tpl`", "Runtime.evaluate"),
        ("40 + 2", "Runtime.evaluate"),
        ("const x = ", "Runtime.evaluate"),
        ("await session.use(tabs[0]", "逗号"),
        ("const o = {a 1}", "键 a"),
        ("const s2 = \"unclosed", "引号"),
    ];
    for (src, must_contain) in bad {
        let e = err_of(src).await;
        assert!(e.contains("行"), "{src} 的错误应带位置: {e}");
        assert!(e.contains("下一步："), "{src} 的错误应带 CTA: {e}");
        assert!(
            e.contains(must_contain),
            "{src} 的错误应含 {must_contain}: {e}"
        );
    }
}

#[tokio::test]
async fn eval_errors_carry_next_step() {
    // 未定义变量
    let e = err_of("return missing_var").await;
    assert!(e.contains("未定义变量 missing_var"), "{e}");
    assert!(e.contains("const missing_var ="), "CTA 应给赋值写法: {e}");

    // 未知 session 方法（列宿主面 + CDP 走法）；中文名在解析层就被拒（合理），
    // 用 ASCII 名触发求值层分支
    let e = err_of("await session.bogus()").await;
    assert!(e.contains("未知 session.bogus"), "{e}");
    assert!(e.contains("peekEvents"), "应列出宿主面: {e}");
    assert!(e.contains("<Domain>.<method>"), "应给 CDP 走法: {e}");

    // 未知全局函数（列可用全局）
    let e = err_of("await nosuchglobal()").await;
    assert!(e.contains("未知函数 nosuchglobal"), "{e}");
    assert!(e.contains("listPageTargets()"), "应列出可用全局: {e}");
}

#[tokio::test]
async fn use_without_target_id_gives_copyable_example() {
    // 求值到参数校验就报错，无需连接
    let e = err_of("await session.use(123)").await;
    assert!(e.contains("下一步：session.use(tabs[0].targetId)"), "{e}");
}
