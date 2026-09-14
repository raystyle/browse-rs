//! 连接线索解析的契约测试（纯函数，无网络）。

use cdp::{parse_port, ws_from_active_port_text};

#[test]
fn parses_bare_port() {
    assert_eq!(parse_port("9222"), Some(9222));
}

#[test]
fn parses_port_from_http_url() {
    assert_eq!(parse_port("http://127.0.0.1:9222"), Some(9222));
    assert_eq!(parse_port("http://localhost:9333/"), Some(9333));
}

#[test]
fn rejects_non_ports() {
    assert_eq!(parse_port("not-a-port"), None);
    assert_eq!(parse_port("http://127.0.0.1"), None);
}

#[test]
fn ws_from_wellformed_active_port_text() {
    let text = "52345\n/devtools/browser/8f4eaaaa-1111-2222\n";
    assert_eq!(
        ws_from_active_port_text(text).as_deref(),
        Some("ws://127.0.0.1:52345/devtools/browser/8f4eaaaa-1111-2222")
    );
}

#[test]
fn ws_from_malformed_active_port_text() {
    assert_eq!(ws_from_active_port_text(""), None);
    assert_eq!(ws_from_active_port_text("12345\n"), None);
    assert_eq!(ws_from_active_port_text("12345\n/not-devtools/x"), None);
}

#[test]
fn ws_from_active_port_text_tolerates_crlf() {
    let text = "52345\r\n/devtools/browser/abc\r\n";
    assert!(ws_from_active_port_text(text).is_some());
}
