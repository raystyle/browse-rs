//! CLI argv 契约锁（#53）：0.12.1 的 G-F 收口把 `next` 必值口改成直出
//! exit 2 后，七处子命令旗标收集循环按「参数尽返 Err 收尾」旧契约写，
//! 全部假报用法错（`fetch` / `artifact` 三兄弟 / `ledger keygen` /
//! `issue new` / `issue list`），`snippets list` 的可选位也一并踩碎。
//! 本件用真二进制（`CARGO_BIN_EXE_browse`）锁双口分面：必值口缺值
//! exit 2，收集口参数尽即收尾。全部用例零网络（dry-run 与本地校验面）。

use std::process::{Command, Stdio};

/// 跑真二进制：stdin 恒 null（issue new 的空 body 会读管道，挂住测试）。
fn run(args: &[&str], env: &[(&str, &str)]) -> (i32, String, String) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_browse"));
    c.args(args).stdin(Stdio::null());
    for (k, v) in env {
        c.env(k, v);
    }
    let out = c.output().expect("browse 二进制在位");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// BROWSE_NAME 传绝对路径即整仓 state_dir（G7 口径）：测试态隔离不碰
/// 本机 ~/.browse-rs（keygen 会写真密钥、snippets 读真库）。Drop 收渣
/// （#53 评审 G-lite：反复跑不留临时目录）。
struct TempState(String);

impl TempState {
    fn new(tag: &str) -> Self {
        let d = std::env::temp_dir().join(format!(
            "browse-arg-contract-{}-{tag}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&d).expect("建临时 state 目录");
        Self(d.to_string_lossy().into_owned())
    }

    fn path(&self) -> &str {
        &self.0
    }
}

impl Drop for TempState {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 七处旗标收集循环（含 `while let Some` 与 `loop { match }` 两形态）与
/// 可选位全部正常收尾，不再假报「旗标 需要一个值」。
#[test]
fn flag_collection_loops_terminate() {
    // issue new：循环走完进 dry-run 本地预览（零网络零签名）
    let (code, out, err) = run(
        &["issue", "new", "t", "--acceptance", "a", "--dry-run"],
        &[],
    );
    assert_eq!(code, 0, "issue new dry-run 应 0：{err}");
    assert!(out.contains("\"dryRun\": true"), "dry-run 回执：{out}");

    // artifact publish：循环走完命中 --kind 必填校验（非旗标缺值假报）
    let (code, _, err) = run(&["artifact", "publish", "--name", "x"], &[]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("--kind 必填"), "应命中 kind 校验：{err}");

    // snippets list 裸调：可选位 [site] 缺省列全库（隔离态空库，空库提
    // 示在 stderr）
    let snips = TempState::new("snips");
    let (code, _, err) = run(&["snippets", "list"], &[("BROWSE_NAME", snips.path())]);
    assert_eq!(code, 0, "裸 snippets list 应 0");
    assert!(err.contains("片段库为空"), "空库回执：{err}");

    // ledger keygen 裸调：循环收尾后真落钥（隔离态，勿碰本机密档）
    let key = TempState::new("key");
    let (code, out, err) = run(&["ledger", "keygen"], &[("BROWSE_NAME", key.path())]);
    assert_eq!(code, 0, "keygen 应 0：{err}");
    assert!(out.contains("\"ok\": true"), "keygen 回执：{out}");

    // fetch / artifact attest / issue list 的循环首迭代（坏旗标与坏值都
    // 在网络前拦截）
    let (code, _, err) = run(&["fetch", "x", "--bogus"], &[]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("参数不认识 --bogus"), "{err}");
    let (code, _, err) = run(&["issue", "list", "--limit", "abc"], &[]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("--limit 要数字"), "{err}");

    // artifact publish 的 outcome 枚举（结构化字段，网络前拦截）
    let (code, _, err) = run(
        &[
            "artifact",
            "publish",
            "--name",
            "x",
            "--kind",
            "lesson",
            "--digest",
            "sha256:0000000000000000000000000000000000000000000000000000000000000000",
            "--outcome",
            "bogus",
        ],
        &[],
    );
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("--outcome 只认 success|failure"), "{err}");
}

/// 必值口（Args::next）缺值仍直出 exit 2——分面只救收集口，不放松守卫。
#[test]
fn required_value_face_still_exits_2() {
    let (code, _, err) = run(&["--eval"], &[]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("--eval 需要一个值"), "{err}");

    let (code, _, err) = run(&["fetch"], &[]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("fetch <url> 需要一个值"), "{err}");
}

/// G-F 的 Mode 层缺参处理器复活（0.12.1 里被解析层假报先拦成死代码）；
/// snippets show 从无守卫（#53 评审 F1），本批补齐同款。
#[test]
fn mode_level_missing_param_reachable() {
    let (code, _, err) = run(&["workspace", "site"], &[]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("workspace site 缺段"), "{err}");

    let (code, _, err) = run(&["snippets", "show"], &[]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("snippets show 缺 rel"), "{err}");
}

/// bail_arg 措辞中性化：非 issue 子命令的坏旗标不再误报「issue 参数不
/// 认识」。
#[test]
fn bail_arg_wording_is_subcommand_neutral() {
    let (code, _, err) = run(&["artifact", "attest", "id-x", "--bogus"], &[]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("参数不认识 --bogus"), "{err}");
    assert!(!err.contains("issue 参数不认识"), "{err}");
}
