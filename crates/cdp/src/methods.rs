//! CDP 命令方法清单与相近建议。
//!
//! `methods.txt` 由 `tools/gen-cdp-methods.py` 从上游
//! `browser_protocol.json` + `js_protocol.json` 生成（652 条，跳过 events
//! 与 redirect 别名，对齐 browser-harness-js `sdk/gen.ts` 口径），随源码
//! 入库；升协议版本后重跑生成脚本再提交。
//!
//! 用途是**被动增强**：不拦截未知方法（避免清单滞后误伤真方法），只在
//! CDP 报 `not found` 时给相近建议，以及给方言 `cdpMethods(domain)` 做
//! 运行时探针（对齐 bh 的 `Object.keys(session.Network)`）。

/// 全量 CDP 命令清单，`Domain.Method` 每行一条（652 条）。
pub const METHODS_RAW: &str = include_str!("methods.txt");

/// 判断方法是否为清单内已知命令（被动增强用，不预拦未知方法）。
///
/// # Examples
///
/// ```
/// assert!(cdp::methods::method_exists("Page.navigate"));
/// assert!(!cdp::methods::method_exists("Page.navigateX"));
/// ```
pub fn method_exists(method: &str) -> bool {
    METHODS_RAW.lines().any(|l| l == method)
}

/// 返回某域的全部命令，是 `cdpMethods("Network")` 的底座。
///
/// # Examples
///
/// ```
/// assert!(cdp::methods::methods_of_domain("Network").contains(&"Network.enable"));
/// assert!(cdp::methods::methods_of_domain("NoSuchDomain").is_empty());
/// ```
pub fn methods_of_domain(domain: &str) -> Vec<&'static str> {
    let prefix = format!("{domain}.");
    METHODS_RAW
        .lines()
        .filter(|l| l.starts_with(&prefix))
        .collect()
}

/// 相近建议：同域优先，方法名前缀/包含次之，最多 `max` 条。
///
/// # Examples
///
/// ```
/// let s = cdp::methods::suggest("Page.navigateX", 3);
/// assert!(s.contains(&"Page.navigate"));
/// ```
pub fn suggest(method: &str, max: usize) -> Vec<&'static str> {
    let (domain, name) = method.split_once('.').unwrap_or((method, ""));
    let same_domain_prefix: Vec<&str> = METHODS_RAW
        .lines()
        .filter(|l| {
            l.starts_with(&format!("{domain}."))
                && l.rsplit('.').next().is_some_and(|n| n.starts_with(name))
        })
        .take(max)
        .collect();
    if same_domain_prefix.len() >= max {
        return same_domain_prefix;
    }
    let mut out = same_domain_prefix;
    for l in METHODS_RAW.lines() {
        if out.len() >= max {
            break;
        }
        if !out.contains(&l) {
            let lname = l.rsplit('.').next().unwrap_or(l);
            if lname.contains(name) || name.contains(lname) {
                out.push(l);
            }
        }
    }
    out
}
