//! 命令面目录契约（incur-rs 原则）：`surface::COMMANDS` 是唯一真相，
//! `docs/surface/` 的 schema/llms 是派生物，本测试锁两层漂移：
//!
//! 1. 盘上产物与渲染器输出逐字节一致（改目录必须 `browse --gen-surface`）。
//! 2. 目录里的每个全局函数/session 方法都有真实派发臂（调用形 eval，
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
            // 调用形才走真派发（裸名只撞变量表，断言会空转假绿）
            CmdKind::Global => format!("{name}()", name = c.name),
            CmdKind::Session => format!("session.{name}()", name = c.name),
            CmdKind::Cli => continue, // CLI 形态不走方言派发
        };
        // chromeUpdate 探针要隔离网络：钉死环回拒连镜像（进程内无其他读者，
        // 本测独占该 env 窗口）
        if c.name == "chromeUpdate" {
            // SAFETY: 测试进程内短窗设置，探针后即刻还原；无并发读者（见上注）
            unsafe {
                std::env::set_var("BROWSE_CHROME_MIRROR", "http://127.0.0.1:9");
            }
        }
        let r = host.eval_snippet(&call).await;
        if c.name == "chromeUpdate" {
            // SAFETY: 同上，还原窗口
            unsafe {
                std::env::remove_var("BROWSE_CHROME_MIRROR");
            }
        }
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

/// 帮助面漂移守卫（cli-docs 第四节）：`render_help` 必须行首精确覆盖目录
/// 全部 CLI 条目（防前缀超串假绿），Commands 节行数等于目录命令条数，头行
/// 版本从载体注入。
#[test]
fn help_covers_catalog() {
    let help = surface::render_help();
    let cli_cmds = surface::COMMANDS
        .iter()
        .filter(|c| c.kind == CmdKind::Cli && !c.signature.starts_with("browse --"))
        .count();
    let cmd_lines = help
        .lines()
        .skip_while(|l| !l.starts_with("Commands:"))
        .skip(1)
        .take_while(|l| !l.starts_with("Options:"))
        .filter(|l| !l.trim().is_empty())
        .count();
    assert_eq!(cmd_lines, cli_cmds, "Commands 节行数应等于目录命令条数");
    for c in surface::COMMANDS.iter().filter(|c| c.kind == CmdKind::Cli) {
        let needle = if c.signature.starts_with("browse --") {
            c.signature.split(' ').nth(1).expect("旗标 token")
        } else {
            c.signature.split(" [").next().expect("命令名")
        };
        let hit = help.lines().map(str::trim_start).any(|l| {
            l == needle
                || l.starts_with(&format!("{needle} "))
                || l.starts_with(&format!("{needle}  "))
        });
        assert!(hit, "帮助面缺 CLI 条目 {needle}（{name}）", name = c.name);
    }
    assert!(
        help.contains(&format!("browse@{}", env!("CARGO_PKG_VERSION"))),
        "帮助面头行版本未注入"
    );
}

/// 反查守卫：main.rs 解析面认识的全部长旗标必须出现在帮助面（维护件与
/// 子命令私有旗标在豁免表）；防新增全局旗标漏进 Options 节。
#[test]
fn help_lists_every_cli_flag() {
    let help = surface::render_help();
    let src = std::fs::read_to_string(repo_root().join("crates/browse-cli/src/main.rs"))
        .expect("main.rs 应可读");
    let exempt = ["--gen-surface", "--body", "--tool", "--status", "--limit"];
    let mut flags: Vec<String> = src
        .split(['"', '|'])
        .map(str::trim)
        .filter(|t| {
            t.starts_with("--")
                && t.len() > 2
                && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
                && !exempt.contains(t)
        })
        .map(str::to_string)
        .collect();
    flags.sort();
    flags.dedup();
    assert!(!flags.is_empty(), "旗标抽取不应为空（抽取器坏了）");
    for f in flags {
        let hit = help.lines().map(str::trim_start).any(|l| {
            l == f
                || l.starts_with(&format!("{f} "))
                || l.starts_with(&format!("{f},"))
                || l.starts_with(&format!("{f}  "))
        });
        assert!(hit, "帮助面缺旗标 {f}");
    }
}
