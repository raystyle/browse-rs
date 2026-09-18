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

/// 模板字符串 raw 语义（#18）：反斜杠序列逐字保留，页面代码原样进 V8。
#[test]
fn template_strings_keep_backslashes_verbatim() {
    use serde_json::json;
    let stmts = parse_script(r"const s = `/\n/g`").unwrap();
    let Stmt::Let { expr, .. } = &stmts[0] else {
        panic!("应是声明");
    };
    assert!(
        matches!(expr, Expr::Lit(v) if v == &json!("/\\n/g")),
        "{expr:?}"
    );
    // 普通字符串的丢字修：\d 保留反斜杠，\n\t 与引号、反斜杠照常转义
    let stmts = parse_script(r#"const r = "\d+\s""#).unwrap();
    let Stmt::Let { expr, .. } = &stmts[0] else {
        panic!("应是声明");
    };
    assert!(
        matches!(expr, Expr::Lit(v) if v == &json!(r"\d+\s")),
        "{expr:?}"
    );
}

/// 模板字符串多行与内层语法（#18）：真换行合法、`//` 不当注释剥、
/// 单双引号免转义、`${}` 是字面量、`` \` `` 转义出反引号。
#[test]
fn template_strings_multiline_and_inner_syntax() {
    use serde_json::json;
    let src = "const t = `first\nvisit \"q\" // stay\n${x} a\\`b`";
    let stmts = parse_script(src).unwrap();
    let Stmt::Let { expr, .. } = &stmts[0] else {
        panic!("应是声明");
    };
    assert!(
        matches!(expr, Expr::Lit(v) if v == &json!("first\nvisit \"q\" // stay\n${x} a`b")),
        "{expr:?}"
    );
}

/// 模板字符串与两追踪器（#18）：strip_comments/depth_ok 认反引号，
/// 模板里的括号不计深度、未闭合反引号视为不完整；闭合奇偶判定与
/// 解析器同源（评审 F 复现件：偶数反斜杠后的反引号照常闭合）。
#[test]
fn template_strings_trackers_are_backtick_aware() {
    use browse_core::parser::strip_comments;
    assert_eq!(strip_comments("`a // b`"), "`a // b`");
    assert!(snippet_complete("const t = `a ( b`"));
    assert!(!snippet_complete("const t = `a ( b"));
    // 奇偶：`a\\`（双反斜杠）后是闭合反引号，尾注释照剥
    assert_eq!(strip_comments("return `a\\\\` // 注"), "return `a\\\\` ");
    assert!(snippet_complete("const s = `a\\\\`"));
    // `a\`（单反斜杠转义了反引号）才是未闭合
    assert!(!snippet_complete("const s = `a\\`"));
}

/// 闭合奇偶矩阵（#18 用户令严格测试）：0 至 5 个反斜杠紧邻反引号，
/// 解析器取值与配平判定逐格对账——偶数闭合（反斜杠全保留），奇数把
/// 反引号转义（整体未闭合）。本矩阵是消转义与闭合判定同口径的守门：
/// 改 parse_string 或 scan_raw_close 时先过这组再谈其他。
#[test]
fn template_close_parity_matrix() {
    use serde_json::json;
    for bs in 0..=5usize {
        let src = format!("return `a{}`", "\\".repeat(bs));
        if bs % 2 == 0 {
            // 偶数：闭合，值 = a + bs 个反斜杠
            let stmts = parse_script(&src).unwrap_or_else(|e| panic!("{bs}: {e}"));
            let Stmt::Return(expr) = &stmts[0] else {
                panic!()
            };
            let expect = format!("a{}", "\\".repeat(bs));
            assert!(matches!(expr, Expr::Lit(v) if v == &json!(expect)), "{bs}");
            assert!(snippet_complete(&format!(
                "const s = `a{}`",
                "\\".repeat(bs)
            )));
        } else {
            // 奇数：反引号被转义，整体未闭合
            assert!(parse_script(&src).is_err(), "{bs} 应未闭合");
            assert!(!snippet_complete(&src));
        }
    }
    // 三个反斜杠 + 反引号 + b + 收尾：前两根字面量、末根转义反引号
    let src3 = "return `a\\\\\\`b`";
    let stmts = parse_script(src3).unwrap();
    let Stmt::Return(expr) = &stmts[0] else {
        panic!()
    };
    assert!(
        matches!(expr, Expr::Lit(v) if v == &json!("a\\\\`b")),
        "{expr:?}"
    );
}

/// 普通字符串转义全表（#18）：\n\t\\与两种引号照常转义，其余保留反斜杠。
#[test]
fn normal_string_escape_matrix() {
    let direct = [
        (r#"return "a\nb""#, "a\nb"),
        (r#"return "a\tb""#, "a\tb"),
        (r#"return "a\\b""#, "a\\b"),
        (r#"return "a\"b""#, "a\"b"),
        ("return 'a\\'b'", "a'b"),
        (r#"return "\d\s\w""#, r"\d\s\w"),
        (r#"return "a\`b""#, "a\\`b"),
    ];
    for (src, expect) in direct {
        let stmts = parse_script(src).unwrap();
        let Stmt::Return(expr) = &stmts[0] else {
            panic!()
        };
        assert!(
            matches!(expr, Expr::Lit(v) if v == &serde_json::json!(expect)),
            "{src} -> {expr:?}"
        );
    }
}

/// 模板 \${ 与 \$ 矩阵（#18）：紧邻奇偶决定降格与否。
#[test]
fn template_dollar_matrix() {
    use serde_json::json;
    let cases = [
        ("return `x${y}z`", "x${y}z"),
        ("return `x\\${y}z`", "x${y}z"),
        ("return `x\\\\${y}z`", "x\\\\${y}z"),
        ("return `$`", "$"),
        ("return `\\$`", "\\$"),
    ];
    for (src, expect) in cases {
        let stmts = parse_script(src).unwrap();
        let Stmt::Return(expr) = &stmts[0] else {
            panic!()
        };
        assert!(
            matches!(expr, Expr::Lit(v) if v == &json!(expect)),
            "{src} -> {expr:?}"
        );
    }
}
