//! 命令面目录契约（incur-rs 原则）：`surface::COMMANDS` 是唯一真相，
//! `docs/surface/` 的 schema/llms 是派生物，本测试锁两层漂移：
//!
//! 1. 盘上产物与渲染器输出逐字节一致（改目录必须 `browse --gen-surface`）。
//! 2. 目录里的每个全局函数/session 方法都有真实派发臂（eval 裸名，
//!    错误不许是「未知函数」；不需要浏览器，走 Not-connected 报错即可证）。

use browse_core::{CmdKind, JsHost, surface};

/// 仓库根（crates/browse-core -> 上两级）。
fn repo_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root")
        .to_path_buf()
}

#[test]
fn generated_files_match_renderer() {
    let dir = repo_root().join("docs").join("surface");
    let schema = serde_json::from_slice::<serde_json::Value>(
        &std::fs::read(dir.join("browse.schema.json"))
            .expect("browse.schema.json 应在（跑 browse --gen-surface docs/surface）"),
    )
    .expect("schema 应是合法 JSON");
    assert_eq!(
        schema,
        surface::render_schema(),
        "schema 漂移：重跑 browse --gen-surface docs/surface"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("llms.txt")).expect("llms.txt 应在"),
        surface::render_llms(),
        "llms.txt 漂移"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("llms-full.txt")).expect("llms-full.txt 应在"),
        surface::render_llms_full(),
        "llms-full.txt 漂移"
    );
    // 技能（SKILL.md）物种已退役：agent 说明书由 browse --llms 直出承担，
    // 目录若有残留 skills/ 即违规（防旧产物混入提交）。
    assert!(
        !dir.join("skills").exists(),
        "docs/surface/skills 应已退役删除"
    );
}

/// REQ-060 一面手册帽（族标准名 --llms）：手册至多 120 行，膨胀逼收敛。
#[test]
fn manual_under_120_lines() {
    let lines = surface::render_manual().lines().count();
    assert!(
        lines <= 120,
        "--llms 手册 {lines} 行超 120 帽；收敛目录说明或精选常用例"
    );
}

#[test]
fn catalog_names_unique() {
    let mut seen = std::collections::HashSet::new();
    for c in surface::COMMANDS {
        assert!(
            seen.insert(format!("{:?}:{}", c.kind, c.name)),
            "目录重名：{}",
            c.name
        );
    }
}

#[tokio::test]
async fn every_catalog_entry_dispatches() {
    let host = JsHost::new(cdp::Session::new());
    for c in surface::COMMANDS {
        let call = match c.kind {
            CmdKind::Global => c.name.to_string(),
            CmdKind::Session => format!("session.{name}", name = c.name),
            CmdKind::Cli => continue, // CLI 形态不走方言派发
        };
        let r = host.eval_snippet(&call).await;
        let msg = match &r {
            Ok(_) => continue, // 有返回值必是真实臂（如 hostFunctions）
            Err(e) => format!("{e:#}"),
        };
        assert!(
            !msg.contains("未知函数") && !msg.contains("未知 session"),
            "目录里的 {} 没有派发臂（或目录名写错）：{msg}",
            c.name
        );
    }
}

/// 帮助面漂移守卫（cli-docs 第四节）：`render_help` 必须覆盖目录全部 CLI
/// 条目（命令形态取截断名、旗标形态取旗标 token），且头行版本从载体注入。
#[test]
fn help_covers_catalog() {
    let help = surface::render_help();
    for c in surface::COMMANDS.iter().filter(|c| c.kind == CmdKind::Cli) {
        let needle = if c.signature.starts_with("browse --") {
            c.signature.split(' ').nth(1).expect("旗标 token")
        } else {
            c.signature.split(" [").next().expect("命令名")
        };
        assert!(
            help.contains(needle),
            "帮助面缺 CLI 条目 {needle}（{name}）",
            name = c.name
        );
    }
    assert!(
        help.contains(&format!("browse@{}", env!("CARGO_PKG_VERSION"))),
        "帮助面头行版本未注入"
    );
}
