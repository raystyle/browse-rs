//! 实例命名空间（多实例，ADR-0006）：`BROWSE_NAME` 一个名字同时决定
//! 状态目录与 daemon 端口。
//!
//! 与 bh 的 `BH_NAME` 同型：默认实例一切照旧（`~/.browse-rs`、9880）；
//! 命名实例的状态（daemon 日志、drops、screenshots、录制、engine-profile）
//! 全进 `~/.browse-rs/<name>/`，端口由名字稳定派生（9900-9999）——
//! 引擎 profile 独占，命名实例可各自 spawn chrome 互不锁。

use std::path::PathBuf;

/// 命名实例缺省端口下界（派生区间 9900..=9999，避开默认 9880 与 bh 的 9876）。
pub const DERIVED_PORT_MIN: u16 = 9900;

/// 实例名：`BROWSE_NAME` 的非空值；未设即默认实例（`None`）。
///
/// # Examples
///
/// ```
/// // 未设 BROWSE_NAME 时是默认实例
/// if std::env::var_os("BROWSE_NAME").is_none() {
///     assert_eq!(browse_core::paths::instance_name(), None);
/// }
/// ```
pub fn instance_name() -> Option<String> {
    std::env::var_os("BROWSE_NAME")
        .map(|v| v.to_string_lossy().trim().to_string())
        .filter(|s| !s.is_empty())
}

/// 按名取状态目录（纯函数，可单测）。
///
/// # Examples
///
/// ```
/// let d = browse_core::paths::state_dir_for(&Some("t1".into()));
/// assert!(d.ends_with(r".browse-rs\t1") || d.ends_with(".browse-rs/t1"));
/// ```
pub fn state_dir_for(name: &Option<String>) -> PathBuf {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    match name {
        Some(n) => home.join(".browse-rs").join(n),
        None => home.join(".browse-rs"),
    }
}

/// 本实例状态目录：`%USERPROFILE%\.browse-rs[\<name>]`。
pub fn state_dir() -> PathBuf {
    state_dir_for(&instance_name())
}

/// FNV-1a（64 位）——名字到端口的稳定散列。
fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 名字稳定派生端口（9900..=9999）。同名恒同口（重启不变）；
/// 异名可能碰撞（~1% 概率），撞上用 `BROWSE_PORT` 显式指定。
///
/// # Examples
///
/// ```
/// let p = browse_core::paths::derived_port("worker-1");
/// assert!((9900..=9999).contains(&p));
/// assert_eq!(p, browse_core::paths::derived_port("worker-1"), "同名同口");
/// assert_ne!(p, browse_core::paths::derived_port("worker-2"));
/// ```
pub fn derived_port(name: &str) -> u16 {
    DERIVED_PORT_MIN + (fnv1a(name) % 100) as u16
}

/// 本实例 daemon 端口：`BROWSE_PORT` 显式优先，其次按 `BROWSE_NAME`
/// 派生，缺省 9880。
///
/// # Examples
///
/// ```
/// // 未设任何变量时是默认口
/// if std::env::var_os("BROWSE_PORT").is_none() && std::env::var_os("BROWSE_NAME").is_none() {
///     assert_eq!(browse_core::paths::daemon_port(), 9880);
/// }
/// ```
pub fn daemon_port() -> u16 {
    if let Some(p) =
        std::env::var_os("BROWSE_PORT").and_then(|v| v.to_string_lossy().parse::<u16>().ok())
    {
        return p;
    }
    instance_name().as_deref().map(derived_port).unwrap_or(9880)
}

/// 本实例引擎 profile：`<state>/engine-profile`（命名实例各自独占，
/// 可同时 spawn chrome 不互锁）。
///
/// # Examples
///
/// ```
/// let dir = browse_core::paths::engine_profile_dir();
/// assert!(dir.ends_with("engine-profile"));
/// ```
pub fn engine_profile_dir() -> PathBuf {
    state_dir().join("engine-profile")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 派生端口：区间内、同名同口、随名散布。
    #[test]
    fn derived_port_spreads() {
        let mut seen = std::collections::HashSet::new();
        for n in ["a", "b", "worker-1", "worker-2", "google", "banks"] {
            let p = derived_port(n);
            assert!((DERIVED_PORT_MIN..DERIVED_PORT_MIN + 100).contains(&p));
            assert_eq!(p, derived_port(n));
            seen.insert(p);
        }
        assert!(seen.len() >= 2, "不同名应散布到不同口");
    }

    /// 状态目录：命名实例带子目录，默认不带孩子。
    #[test]
    fn state_dir_namespacing() {
        let named = state_dir_for(&Some("x".into()));
        let default = state_dir_for(&None);
        assert!(named.starts_with(&default), "命名目录在默认目录之下");
        assert!(named.ends_with("x"));
        assert_eq!(
            default.components().next_back(),
            named.components().nth_back(1),
            "默认目录是命名目录的父级"
        );
    }
}
