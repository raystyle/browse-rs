//! 方言语法契约：支持的构造能解析，方言外构造在解析期就报错并带提示。

use browse_core::parser::{Expr, Stmt, parse_script, render};
use browse_core::snippet_complete;

#[test]
fn parses_sample_readme_snippets() {
    // 样例 README 的四个片段必须都能解析
    for src in [
        "const tabs = await listPageTargets()",
        "await session.use(tabs[0].targetId)",
        r#"await session.Page.navigate({url:"https://example.com"})"#,
        r#"await session.waitFor("Page.loadEventFired", undefined, 15000)"#,
    ] {
        assert!(parse_script(src).is_ok(), "应可解析: {src}");
    }
}

#[test]
fn parses_every_construct() {
    let stmts = parse_script(
        "const o = {a: 1, b: [true, null, 'x']}\nreturn await session.Page.navigate(o)",
    )
    .unwrap();
    assert_eq!(stmts.len(), 2);
    assert!(matches!(&stmts[0], Stmt::Let { name, .. } if name == "o"));
    assert!(matches!(&stmts[1], Stmt::Return(Expr::Await(_))));
}

#[test]
fn rejects_if_for_functions_with_hint() {
    for src in [
        "if (x) { await session.Page.navigate({}) }",
        "for (const t of tabs) { print(t) }",
        "const f = (x) => x + 1",
        "function go() { return 1 }",
        "const s = `tpl ${x}`",
    ] {
        let err = parse_script(src).unwrap_err().to_string();
        assert!(
            err.contains("Runtime.evaluate"),
            "应提示放 Runtime.evaluate: {src} -> {err}"
        );
    }
}

#[test]
fn rejects_dangling_syntax() {
    assert!(parse_script("const x = ").is_err());
    assert!(parse_script("await session.use(tabs[0]").is_err());
    assert!(parse_script("const o = {a 1}").is_err());
}

#[test]
fn strips_line_comments_not_strings() {
    use browse_core::parser::strip_comments;
    assert_eq!(strip_comments("a // 尾注\nb"), "a \nb");
    assert_eq!(strip_comments(r#""http://x""#), r#""http://x""#);
}

#[test]
fn render_round_trips_canonical_form() {
    let src = r#"await session.Page.navigate({url:"https://example.com"})"#;
    let stmts = parse_script(src).unwrap();
    assert_eq!(
        render(&stmts),
        r#"await session.Page.navigate({url: "https://example.com"})"#
    );
}

#[test]
fn snippet_complete_balances() {
    assert!(snippet_complete("await session.use(tabs[0].targetId)"));
    assert!(!snippet_complete("const x = {a: 1"));
    assert!(!snippet_complete("f(\"unclosed"));
    assert!(!snippet_complete(""));
}

#[test]
fn empty_and_comment_only_scripts_are_empty() {
    assert!(parse_script("").unwrap().is_empty());
    assert!(parse_script("// 只有注释").unwrap().is_empty());
}
