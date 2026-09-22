//! browse：给 agent 用的 browse CLI。
//!
//! 忠实移植 browser-harness-rs 的传输三形态（`--eval` / stdin / TTY REPL），
//! 外加常驻 daemon（首次使用自动拉起）与引擎生命周期子命令：
//!
//! ```text
//! browse 'await session.Page.navigate({url:"https://example.com"})'
//! browse up [--headless]     browse down     browse status
//! ```
//!
//! 引擎策略：附着优先（探测 9222 / DevToolsActivePort），缺则 spawn
//! clean-chrome 专属实例；`browse down` 只终结自己 spawn 的。

use anyhow::{Result, anyhow, bail};
use browse_cli::client;
use browse_core::snippet_complete;
use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;
use serde_json::json;
use std::io::IsTerminal;

/// CLI 入口解析出的形态：缺省求值或生命周期子命令。
enum Mode {
    /// 求值片段（缺省）。
    Eval,
    /// `browse up`：显式起引擎。
    Up,
    /// `browse down`：退 daemon（只杀自起引擎）。
    Down,
    /// `browse status`：看 daemon/引擎状态。
    Status,
    /// `browse update`：browse 自更新（发现加锚校验加自替换；ark 管理拦）。
    Update,
    /// `browse chrome install <版本> [部署目录]`：镜像下载或本地导入安装 Chromium 版本。
    ChromeInstall {
        version: String,
        from_dir: Option<String>,
    },
    /// `browse chrome list`：列已装版本与 pin。
    ChromeList,
    /// `browse chrome use <版本>`：pin 切换。
    ChromeUse(String),
    /// `browse chrome update`：发现最新版，未装则镜像装，pin 切最新。
    ChromeUpdate,
    /// `browse chrome remove <版本>`：删已装版本（pin 指向的拒删）。
    ChromeRemove(String),
    /// `browse chrome doctor`：托管部署体检。
    ChromeDoctor,
    /// `browse issue new <标题> --acceptance <验收> [--kind bug|improvement] [--body <正文>] [--dry-run]`：账本 issue 流开单（REQ-063，真源 ledger.ohmygh.com）。
    IssueNew {
        title: String,
        body: String,
        kind: String,
        acceptance: String,
        dry: bool,
    },
    /// `browse issue list [--limit <n>] [--before <id>]`：列 issue（账本面；count 是返回条数，has_more 权威）。
    IssueList { limit: u32, before: Option<u64> },
    /// `browse issue show <id>`：看 issue 详情。
    IssueShow(String),
    /// `browse snippets list [site]`：列片段库（#44）。
    SnippetsList(Option<String>),
    /// `browse snippets show <rel>`：看片段全文（#44）。
    SnippetsShow(String),
    /// `browse workspace status [--json]`：workspace 仓与 git 状态概览（#50/#51 配套）。
    WorkspaceStatus,
    /// `browse workspace install`：git clone 种子仓到仓根。
    WorkspaceInstall,
    /// `browse workspace update`：git pull --ff-only（脏树拒绝）。
    WorkspaceUpdate,
    /// `browse workspace list`：列在册域名段与 page slug。
    WorkspaceList,
    /// `browse workspace site <段>[/<文件>]`：域技能清单或全文。
    WorkspaceSite(String),
    /// `browse workspace page <slug>`：页面技能全文。
    WorkspacePage(String),
    /// `browse fetch <url> [--markdown] [--timeout <s>]`：一次性只读抓取（#50）。
    Fetch { url: String, timeout_s: u64 },
    /// `browse issue close` 面已移除（总台修正令 2026-09-20 收口：关闭与删除唯一道 = omc 工位经 herdr 委托）。
    /// `browse artifact publish --name <n> --kind <k> --digest <d>`：产物共享库发布（REQ-063；--dep 可重复记依赖出处；--summary/--outcome/--git-sha 结构化字段网页详情直出，参数面随 ledger-client v0.1.3）。
    ArtifactPublish {
        name: String,
        kind: String,
        digest: String,
        version: Option<String>,
        git_range: Option<String>,
        note: Option<String>,
        summary: Option<String>,
        outcome: Option<String>,
        git_sha: Option<String>,
        deps: Vec<String>,
    },
    /// `browse artifact attest <id> --type <t>`：产物验证事件（attest_dev/attest_prod/verification_failed 三型；promote/demote/supersede 归 omc）。
    ArtifactAttest {
        id: String,
        ev_type: String,
        note: Option<String>,
    },
    /// `browse artifact list [--current] [--env dev|prod]`：产物列表。
    ArtifactList {
        current: bool,
        env_f: Option<String>,
    },
    /// `browse ledger keygen [--force]`：Ed25519 密钥对（REQ-063）。
    LedgerKeygen { force: bool },
}

/// 账本调用统一走阻塞线程（收口适配 2026-09-20）：ledger-client 是
/// reqwest blocking 客户端，在 async 上下文里构造/销毁会 panic（嵌套
/// runtime 的 drop 限制，实弹红）；spawn_blocking 让它整个生命周期留在
/// 阻塞线程池。
async fn ledger_call<T, F>(f: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> std::result::Result<T, String> + Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| anyhow!("ledger 线程故障：{e}"))?
        .map_err(|e| anyhow!("{e}"))
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut mode = Mode::Eval;
    let mut snippets: Vec<String> = Vec::new();
    let mut serve = false;
    let mut bind: Option<String> = None;
    let mut connect: Option<String> = None;
    let mut new_tab = false;
    let mut ws: Option<String> = None;
    let mut port: Option<u16> = None;
    let mut chrome: Option<String> = None;
    let mut profile: Option<String> = None;
    let mut headless = false;
    let mut pipe = false;
    let mut json = false;
    let mut js = false;
    let mut b64 = false;
    let mut llms = false;
    let mut full = false;
    let mut repl = false;
    let mut proxy: Option<String> = None;
    let mut proxy_bypass: Option<String> = None;
    let mut engine_args: Vec<String> = Vec::new();
    let mut isolated = false;
    let mut idle_timeout: Option<String> = None;
    let mut cookies_csv: Option<String> = None;
    let mut secrets: Option<String> = None;

    let mut args = Args(std::env::args().skip(1));
    let mut gen_surface: Option<String> = None;
    while let Some(a) = args.next_opt() {
        match a.as_str() {
            "--eval" | "-e" => snippets.push(args.next("--eval")),
            "--gen-surface" => gen_surface = Some(args.next("--gen-surface")),
            "--serve" => serve = true,
            "--bind" => bind = Some(args.next("--bind")),
            "--connect" => connect = Some(args.next("--connect")),
            "--new-tab" => new_tab = true,
            "--ws" => ws = Some(args.next("--ws")),
            "--port" => {
                port = match args.next("--port").parse() {
                    Ok(p) => Some(p),
                    // 用法错直出 exit 2（同 next 闭包口径，评审 G-F）
                    Err(_) => {
                        eprintln!("browse: --port 要数字（退出 2）");
                        std::process::exit(2)
                    }
                }
            }
            "--chrome" => chrome = Some(args.next("--chrome")),
            "--profile" => profile = Some(args.next("--profile")),
            "--headless" => headless = true,
            "--pipe" => pipe = true,
            "--proxy" => proxy = Some(args.next("--proxy")),
            "--proxy-bypass" => proxy_bypass = Some(args.next("--proxy-bypass")),
            // #48：引擎附加旗标直通 spawn（可叠加），值以 -- 开头也原样收
            "--engine-arg" => engine_args.push(args.next("--engine-arg")),
            "--isolated" => isolated = true,
            "--idle-timeout" => idle_timeout = Some(args.next("--idle-timeout")),
            // #48：无头起引擎时从附着浏览器按域克隆 cookie（逗号分隔多域）
            "--cookies" => cookies_csv = Some(args.next("--cookies")),
            "--secrets" => secrets = Some(args.next("--secrets")),
            "--js" => js = true,
            // base64 通道只留长参：`-b` 短参是 issue new 的 --body 既有
            // 契约（REQ-057），不夺权
            "--b64" => b64 = true,
            "--json" => json = true,
            "--llms" => llms = true,
            "--repl" => repl = true,
            "--full" => full = true,
            "-h" | "--help" => {
                print_help();
                return Ok(());
            }
            "-V" | "--version" => {
                println!("browse {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "up" if snippets.is_empty() && mode_is_eval(&mode) => mode = Mode::Up,
            "down" if snippets.is_empty() && mode_is_eval(&mode) => mode = Mode::Down,
            "status" if snippets.is_empty() && mode_is_eval(&mode) => mode = Mode::Status,
            "update" if snippets.is_empty() && mode_is_eval(&mode) => mode = Mode::Update,
            "chrome" if snippets.is_empty() && mode_is_eval(&mode) => {
                match args.next("chrome").as_str() {
                    "list" => mode = Mode::ChromeList,
                    "doctor" => mode = Mode::ChromeDoctor,
                    "use" => mode = Mode::ChromeUse(args.next("chrome use")),
                    "remove" => mode = Mode::ChromeRemove(args.next("chrome remove <版本>")),
                    "update" => mode = Mode::ChromeUpdate,
                    "install" => {
                        let version = args.next("chrome install <版本>");
                        let from_dir = args.next_opt().filter(|s| !s.starts_with('-'));
                        mode = Mode::ChromeInstall { version, from_dir };
                    }
                    other => {
                        eprintln!(
                            "browse: chrome 子命令不认识 {other}（install/use/update/remove/list/doctor，退出 2）"
                        );
                        std::process::exit(2);
                    }
                }
            }
            "fetch" if snippets.is_empty() && mode_is_eval(&mode) => {
                let url = args.next("fetch <url>");
                let mut _markdown = false; // v1 同 text（#50），旗标受理向后兼容
                let mut timeout_s = 15u64;
                while let Some(f) = args.next_opt() {
                    match f.as_str() {
                        "--markdown" | "-m" => _markdown = true,
                        "--timeout" => {
                            timeout_s = args.next("--timeout").parse().unwrap_or_else(|_| {
                                eprintln!("browse: --timeout 要数字（退出 2）");
                                std::process::exit(2);
                            })
                        }
                        other2 => bail_arg(other2),
                    }
                }
                mode = Mode::Fetch { url, timeout_s };
            }
            "artifact" if snippets.is_empty() && mode_is_eval(&mode) => {
                match args.next("artifact").as_str() {
                    "publish" => {
                        // kind 必填（REQ-063 十五枚举无缺省）；--dep 可重复
                        //（deps[] 一等公民，回溯链即证据链）；参数面随标准
                        // crate ledger-client 收口（总台修正令 2026-09-20）
                        let mut name = String::new();
                        let mut kind = String::new();
                        let mut digest = String::new();
                        let mut version = None;
                        let mut git_range = None;
                        let mut note = None;
                        let mut summary = None;
                        let mut outcome = None;
                        let mut git_sha = None;
                        let mut deps: Vec<String> = Vec::new();
                        loop {
                            match args.next_opt() {
                                Some(f) if f == "--name" => name = args.next("--name"),
                                Some(f) if f == "--kind" => kind = args.next("--kind"),
                                Some(f) if f == "--digest" => digest = args.next("--digest"),
                                Some(f) if f == "--version" => {
                                    version = Some(args.next("--version"))
                                }
                                Some(f) if f == "--git-range" => {
                                    git_range = Some(args.next("--git-range"))
                                }
                                Some(f) if f == "--note" => note = Some(args.next("--note")),
                                Some(f) if f == "--dep" => deps.push(args.next("--dep")),
                                // 结构化字段（ledger-client v0.1.3 面）：summary
                                // 一行技术摘要与 outcome 结果倾向，网页详情直出
                                Some(f) if f == "--summary" => {
                                    summary = Some(args.next("--summary"))
                                }
                                Some(f) if f == "--outcome" => {
                                    outcome = Some(args.next("--outcome"))
                                }
                                Some(f) if f == "--git-sha" => {
                                    git_sha = Some(args.next("--git-sha"))
                                }
                                Some(f) => bail_arg(&f),
                                None => break,
                            }
                        }
                        if kind.is_empty() {
                            eprintln!(
                                "browse: --kind 必填（十五枚举：binary/experience/lesson/research/prototype/…，退出 2）"
                            );
                            std::process::exit(2);
                        }
                        if let Some(o) = outcome.as_deref()
                            && !matches!(o, "success" | "failure")
                        {
                            eprintln!(
                                "browse: --outcome 只认 success|failure，得 {o}（用法错，退出 2）"
                            );
                            std::process::exit(2);
                        }
                        // 自由字段本地预检（评审 G1，与 digest/kind/name 同款
                        // 免配额损耗）：summary 非空限长，git_sha 十六进制形
                        if let Some(s) = summary.as_deref() {
                            if s.trim().is_empty() || s.chars().count() > 1000 {
                                eprintln!(
                                    "browse: --summary 要非空且至多 1000 字符（用法错，退出 2）"
                                );
                                std::process::exit(2);
                            }
                        }
                        if let Some(g) = git_sha.as_deref()
                            && (!(7..=40).contains(&g.len())
                                || !g.chars().all(|c| c.is_ascii_hexdigit()))
                        {
                            eprintln!("browse: --git-sha 要 7 至 40 位十六进制（用法错，退出 2）");
                            std::process::exit(2);
                        }
                        mode = Mode::ArtifactPublish {
                            name,
                            kind,
                            digest,
                            version,
                            git_range,
                            note,
                            summary,
                            outcome,
                            git_sha,
                            deps,
                        };
                    }
                    "attest" => {
                        let id = args.next("artifact attest <id>");
                        let mut ev_type = String::from("attest_dev");
                        let mut note = None;
                        loop {
                            match args.next_opt() {
                                Some(f) if f == "--type" => ev_type = args.next("--type"),
                                Some(f) if f == "--note" => note = Some(args.next("--note")),
                                Some(f) => bail_arg(&f),
                                None => break,
                            }
                        }
                        mode = Mode::ArtifactAttest { id, ev_type, note };
                    }
                    "list" => {
                        let mut current = false;
                        let mut env_f = None;
                        loop {
                            match args.next_opt() {
                                Some(f) if f == "--current" => current = true,
                                Some(f) if f == "--env" => env_f = Some(args.next("--env")),
                                Some(f) => bail_arg(&f),
                                None => break,
                            }
                        }
                        mode = Mode::ArtifactList { current, env_f };
                    }
                    other => {
                        eprintln!(
                            "browse: artifact 子命令不认识 {other}（publish/attest/list；promote/demote 面已收口归 omc 工位，退出 2）"
                        );
                        std::process::exit(2);
                    }
                }
            }
            "ledger" if snippets.is_empty() && mode_is_eval(&mode) => {
                match args.next("ledger").as_str() {
                    "keygen" => {
                        let mut force = false;
                        while let Some(f) = args.next_opt() {
                            match f.as_str() {
                                "--force" => force = true,
                                other2 => bail_arg(other2),
                            }
                        }
                        mode = Mode::LedgerKeygen { force };
                    }
                    other => {
                        eprintln!("browse: ledger 子命令不认识 {other}（keygen，退出 2）");
                        std::process::exit(2);
                    }
                }
            }
            "snippets" if snippets.is_empty() && mode_is_eval(&mode) => {
                match args.next("snippets").as_str() {
                    "list" => mode = Mode::SnippetsList(args.next_opt()),
                    "show" => mode = Mode::SnippetsShow(args.next_opt().unwrap_or_default()),
                    other => {
                        eprintln!("browse: snippets 子命令不认识 {other}（list/show，退出 2）");
                        std::process::exit(2);
                    }
                }
            }
            "workspace" if snippets.is_empty() && mode_is_eval(&mode) => {
                match args.next("workspace").as_str() {
                    "status" => mode = Mode::WorkspaceStatus,
                    "install" => mode = Mode::WorkspaceInstall,
                    "update" => mode = Mode::WorkspaceUpdate,
                    "list" => mode = Mode::WorkspaceList,
                    "site" => mode = Mode::WorkspaceSite(args.next_opt().unwrap_or_default()),
                    "page" => mode = Mode::WorkspacePage(args.next_opt().unwrap_or_default()),
                    other => {
                        eprintln!(
                            "browse: workspace 子命令不认识 {other}（status/install/update/list/site/page，退出 2）"
                        );
                        std::process::exit(2);
                    }
                }
            }
            "issue" if snippets.is_empty() && mode_is_eval(&mode) => {
                match args.next("issue").as_str() {
                    "new" => {
                        let title = args.next("issue new <标题>");
                        // --kind（bug|improvement 缺省 bug）加 --acceptance（必
                        // 填，关单 result 的完成判据锚，REQ-063 契约与 hst 家
                        // 族同规）加可选 --body；--dry-run 本地校验加载荷预览
                        // 零网络（#57 G6 评审强制项，账本只增不可撤故保留）
                        let mut body = String::new();
                        let mut kind = String::from("bug");
                        let mut acceptance = String::new();
                        let mut dry = false;
                        loop {
                            match args.next_opt() {
                                Some(f) if f == "--body" || f == "-b" => {
                                    body = args.next("--body");
                                }
                                Some(f) if f == "--kind" => kind = args.next("--kind"),
                                Some(f) if f == "--acceptance" => {
                                    acceptance = args.next("--acceptance")
                                }
                                Some(f) if f == "--dry-run" => dry = true,
                                Some(f) => bail_arg(&f),
                                None => break,
                            }
                        }
                        if acceptance.is_empty() {
                            eprintln!(
                                "browse: --acceptance 必填（关单 result 引 digest 的完成判据；--dry-run 可先预览，退出 2）"
                            );
                            std::process::exit(2);
                        }
                        mode = Mode::IssueNew {
                            title,
                            body,
                            kind,
                            acceptance,
                            dry,
                        };
                    }
                    "list" => {
                        // 默认 100（服务端上限）：默认面即全量，防 open 集超
                        // 20 后旧条目静默隐形（#52）
                        let mut limit = 100u32;
                        let mut before = None;
                        // next_opt 参数尽返 None 即旗标收尾（必值口在
                        // Args::next，分面见 #53）
                        while let Some(f) = args.next_opt() {
                            match f.as_str() {
                                "--before" => {
                                    let b = args.next("--before");
                                    before = Some(b.parse().unwrap_or_else(|_| {
                                        eprintln!("browse: --before 要数字 issue 号（退出 2）");
                                        std::process::exit(2);
                                    }))
                                }
                                "--limit" => {
                                    limit = args.next("--limit").parse().unwrap_or_else(|_| {
                                        eprintln!("browse: --limit 要数字（退出 2）");
                                        std::process::exit(2);
                                    })
                                }
                                other2 => bail_arg(other2),
                            }
                        }
                        mode = Mode::IssueList { limit, before };
                    }
                    "show" => mode = Mode::IssueShow(args.next("issue show <id>")),
                    other => {
                        eprintln!(
                            "browse: issue 子命令不认识 {other}（new/list/show；close 面已收口归 omc 工位，退出 2）"
                        );
                        std::process::exit(2);
                    }
                }
            }
            other if other.starts_with('-') => {
                eprintln!("browse: 未知参数 {other}（用法错，退出 2）");
                std::process::exit(2);
            }
            snippet => snippets.push(snippet.to_string()),
        }
    }

    // --llms（REQ-060 一面，族标准名）：裸形 markdown 紧凑手册（REQ-002 的
    // 目录清单形升册）；--full 人读完整目录；--json 机器形 Schema；不拉 daemon；
    // 优先级 json > full
    if llms {
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&browse_core::surface::render_schema())?
            );
        } else if full {
            print!("{}", browse_core::surface::render_llms_full());
        } else {
            print!("{}", browse_core::surface::render_manual());
        }
        return Ok(());
    }

    if serve {
        let bind = bind.unwrap_or_else(client::daemon_bind);
        return serve_foreground(
            bind,
            ws,
            port,
            chrome.map(Into::into),
            headless,
            pipe,
            profile,
        )
        .await;
    }

    // 维护命令：从 surface 目录重生成 schema/llms（提交 docs/surface/）
    // -b/--b64 通道（#22）：片段实参按 base64 解码（PowerShell 引号与
    // 编码一并绕开；方言与 --js 两形态通用），坏串报 CTA
    if b64 {
        for snip in &mut snippets {
            *snip = browse_cli::client::decode_arg_b64(snip).unwrap_or_else(|e| {
                eprintln!("{e:#}");
                std::process::exit(2);
            });
        }
    }

    if let Some(dir) = gen_surface {
        let p = std::path::PathBuf::from(&dir);
        browse_core::surface::write_surface_files(&p)?;
        println!("surface 已生成到 {}", p.display());
        return Ok(());
    }

    // --connect 映射成显式引擎意图（ws 优先，其次端口）
    if let Some(c) = connect
        && ws.is_none()
        && port.is_none()
    {
        if c.starts_with("ws://") || c.starts_with("wss://") {
            ws = Some(c);
        } else if let Some(p) = cdp::parse_port(&c) {
            port = Some(p);
        } else {
            eprintln!("browse: --connect 认不出 {c}（要 ws://…、端口或 host:port，退出 2）");
            std::process::exit(2);
        }
    }

    match mode {
        Mode::Up => {
            // 生命周期旗标随 daemon 环境注入（#25.1/#25.5）：只影响新拉起
            // 的 daemon，已在跑的以启动时口径为准
            let mut envs: Vec<(&str, String)> = Vec::new();
            if let Some(t) = &idle_timeout {
                envs.push(("BROWSE_IDLE_TIMEOUT", t.clone()));
            }
            if let Some(p) = &proxy {
                envs.push(("BROWSE_PROXY", p.clone()));
            }
            if let Some(b) = &proxy_bypass {
                envs.push(("BROWSE_PROXY_BYPASS", b.clone()));
            }
            // #48：附加旗标空格连接注入 daemon env（值含空格即失真，本
            // 通道只服务无空格 chrome 旗标；含空格场景走 /engine/up 直传）
            if !engine_args.is_empty() {
                envs.push(("BROWSE_ENGINE_ARGS", engine_args.join(" ")));
            }
            if let Some(f) = secrets
                .clone()
                .or_else(|| std::env::var("BROWSE_SECRETS").ok())
            {
                if let Err(e) = browse_core::load_secrets(&f) {
                    eprintln!("{e:#}");
                    std::process::exit(1);
                }
                envs.push(("BROWSE_SECRETS", f));
            }
            // #50/#51 技能层透传（改配置重启 daemon 口径同 BROWSE_SECRETS）
            envs.extend(client::skills_passthrough_env());
            client::ensure_daemon_with_env(&envs).await?;
            let health = client::engine_up(client::UpParams {
                headless,
                chrome,
                ws,
                port,
                pipe,
                profile,
                proxy,
                proxy_bypass,
                isolated,
                engine_args,
            })
            .await?;
            // #48：--cookies 从附着浏览器按域热迁登录态到新引擎（只读源）
            if let Some(csv) = &cookies_csv {
                let domains: Vec<String> = csv
                    .split(',')
                    .map(|d| d.trim().to_string())
                    .filter(|d| !d.is_empty())
                    .collect();
                if !domains.is_empty() {
                    // 方言数组字面量：["a.com","b.com"] 形直接可写
                    let arr = format!(
                        "[{}]",
                        domains
                            .iter()
                            .map(|d| serde_json::to_string(d).unwrap_or_default())
                            .collect::<Vec<_>>()
                            .join(",")
                    );
                    let code = format!("return await cloneCookies({arr})");
                    let r = client::eval(&code, false, false).await?;
                    println!("cookies 克隆：{r}");
                }
            }
            print_health(&health, json);
            Ok(())
        }
        Mode::Down => {
            if client::daemon_alive().await.is_none() {
                println!("daemon 不在跑（无需退出）");
                return Ok(());
            }
            client::quit().await?;
            println!("daemon 退出（自起引擎已随之下线；附着来源浏览器不受影响）");
            Ok(())
        }
        // 自更新：纯本地链（发现加下载加自替换），blocking 全收 spawn_blocking
        Mode::Update => {
            let brief = tokio::task::spawn_blocking(browse_core::self_update::update_self)
                .await
                .map_err(|e| anyhow!("自更新任务崩了：{e}"))??;
            println!("{}", serde_json::to_string_pretty(&brief)?);
            Ok(())
        }
        Mode::Status => {
            match client::daemon_alive().await {
                Some(h) => print_health(&h, json),
                None => println!("daemon 不在跑（跑任意片段或 browse up 自动拉起）"),
            }
            Ok(())
        }
        // Chromium 版本管理器：纯本地操作，不经 daemon（ADR-0007）。
        // 镜像腿的 blocking http client 必须在阻塞线程里生灭（async 上下文
        // drop 它会 panic），与方言侧同式收 spawn_blocking。
        Mode::ChromeInstall { version, from_dir } => {
            let root = browse_core::chrome_mgr::chromium_root();
            let brief = tokio::task::spawn_blocking(move || match from_dir.as_deref() {
                Some(dir) => browse_core::chrome_mgr::install_from_dir(
                    &root,
                    &version,
                    std::path::Path::new(dir),
                ),
                // 部署目录缺省走 R2 镜像下载腿（chrome.ohmygh.com，REQ-003）
                None => browse_core::chrome_mgr::install_from_mirror(&root, &version),
            })
            .await
            .map_err(|e| anyhow!("安装任务崩了：{e}"))??;
            println!("{}", serde_json::to_string_pretty(&brief)?);
            Ok(())
        }
        Mode::ChromeList => {
            println!(
                "{}",
                serde_json::to_string_pretty(&browse_core::chrome_mgr::list_json(
                    &browse_core::chrome_mgr::chromium_root()
                ))?
            );
            Ok(())
        }
        Mode::ChromeUse(v) => {
            let r = browse_core::chrome_mgr::use_version(
                &browse_core::chrome_mgr::chromium_root(),
                &v,
            )?;
            println!("{}", serde_json::to_string_pretty(&r)?);
            Ok(())
        }
        // remove 同为本地重 IO（大版本目录整删），与 update 腿同规收 spawn_blocking
        Mode::ChromeRemove(v) => {
            let root = browse_core::chrome_mgr::chromium_root();
            let brief = tokio::task::spawn_blocking(move || {
                browse_core::chrome_mgr::remove_version(&root, &v)
            })
            .await
            .map_err(|e| anyhow!("删除任务崩了：{e}"))??;
            println!("{}", serde_json::to_string_pretty(&brief)?);
            Ok(())
        }
        // update 腿同样纯本地操作（发现加下载加 pin），blocking 全收 spawn_blocking
        Mode::ChromeUpdate => {
            let root = browse_core::chrome_mgr::chromium_root();
            let brief = tokio::task::spawn_blocking(move || browse_core::chrome_mgr::update(&root))
                .await
                .map_err(|e| anyhow!("升级任务崩了：{e}"))??;
            println!("{}", serde_json::to_string_pretty(&brief)?);
            Ok(())
        }
        Mode::ChromeDoctor => {
            println!(
                "{}",
                serde_json::to_string_pretty(&browse_core::chrome_mgr::doctor_json(
                    &browse_core::chrome_mgr::chromium_root()
                ))?
            );
            Ok(())
        }
        // issue 通道（REQ-063）：真源 ledger.ohmygh.com（旧 issues.ohmygh.com
        // 过渡保役），签名道与只增面在标准 crate ledger-client（总台修正令
        // 2026-09-20 收口）；关闭/删除唯一道 = omc 工位经 herdr 委托
        Mode::IssueNew {
            title,
            body,
            kind,
            acceptance,
            dry,
        } => {
            let mut body = body;
            if body.is_empty() && !std::io::stdin().is_terminal() {
                tokio::io::AsyncReadExt::read_to_string(&mut tokio::io::stdin(), &mut body).await?;
            }
            // --dry-run（#57 G6）：本地同规校验加载荷与签名基预览，零网络
            // 零签名——账本只增不可撤，误发测试单比旧面更贵
            if dry {
                let r =
                    browse_cli::ledger::issue_open_dry_run(&title, &kind, &acceptance, Some(&body))
                        .map_err(|e| anyhow!("{e}"))?;
                println!("{}", serde_json::to_string_pretty(&r)?);
                return Ok(());
            }
            // 实发腿同规预检（收口批评审 F1）：与 --dry-run 同一道本地校验，
            // 坏入参 exit 2 不打网络（服务端 400 也损耗 per-key 日配额）
            if let Err(e) = browse_cli::ledger::validate_issue_open(&title, &kind) {
                eprintln!("browse: {e}（用法错，退出 2）");
                std::process::exit(2);
            }
            let n = ledger_call(move || {
                let n = browse_cli::ledger::client()?
                    .issue_new(&title, &kind, &acceptance, Some(&body))
                    .map_err(|e| e.to_string())?;
                // crate 静默默认守卫（评审 G4）：回执缺 issue 字段时 as_u64
                // 落 0（如幂等命中回执无该键），零即报错防假成功
                if n == 0 {
                    return Err("ledger 回执缺 issue 号（0），拒假成功".into());
                }
                Ok(n)
            })
            .await?;
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({ "ok": true, "issue": n }))?
            );
            Ok(())
        }
        Mode::IssueList { limit, before } => {
            let eff = browse_cli::ledger::clamp_issue_limit(limit);
            let r = ledger_call(move || {
                browse_cli::ledger::client()?
                    .issue_list(eff, before)
                    .map_err(|e| e.to_string())
            })
            .await?;
            let rows = r["issues"].as_array().cloned().unwrap_or_default();
            if r["has_more"]
                .as_bool()
                .unwrap_or_else(|| browse_cli::ledger::issue_list_saturated(rows.len(), eff))
            {
                eprintln!("{}", browse_cli::ledger::issue_list_truncation_hint(eff));
            }
            println!("{}", serde_json::to_string_pretty(&r)?);
            Ok(())
        }
        Mode::IssueShow(id) => {
            let n: u64 = id
                .parse()
                .map_err(|_| anyhow!("issue 号须数字（得 {id}）"))?;
            let r = ledger_call(move || {
                browse_cli::ledger::client()?
                    .issue_show(n)
                    .map_err(|e| e.to_string())
            })
            .await?;
            println!("{}", serde_json::to_string_pretty(&r)?);
            Ok(())
        }
        Mode::ArtifactPublish {
            name,
            kind,
            digest,
            version,
            git_range,
            note,
            summary,
            outcome,
            git_sha,
            deps,
        } => {
            // 值域/形错统一用法错 exit 2（评审 G5，与退出码契约「2=用法错」对齐）
            if let Err(e) = browse_cli::ledger::validate_digest(&digest) {
                eprintln!("browse: {e}（用法错，退出 2）");
                std::process::exit(2);
            }
            if !ledger_client::ARTIFACT_KINDS.contains(&kind.as_str()) {
                eprintln!(
                    "browse: kind 仅十五枚举（ledger-client 同源表），得 {kind}（用法错，退出 2）"
                );
                std::process::exit(2);
            }
            if let Err(e) = browse_cli::ledger::validate_artifact_publish(&name) {
                eprintln!("browse: {e}（用法错，退出 2）");
                std::process::exit(2);
            }
            let id = ledger_call(move || {
                let id = browse_cli::ledger::client()?
                    .artifact_publish_full(
                        &name,
                        &kind,
                        &digest,
                        version.as_deref(),
                        git_range.as_deref(),
                        &deps,
                        note.as_deref(),
                        summary.as_deref(),
                        outcome.as_deref(),
                        git_sha.as_deref(),
                    )
                    .map_err(|e| e.to_string())?;
                // crate 静默默认守卫（评审 G4）：回执缺 artifact_id 时落空串
                if id.is_empty() {
                    return Err("ledger 回执缺 artifact_id（空），拒假成功".into());
                }
                Ok(id)
            })
            .await?;
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({ "ok": true, "artifact_id": id }))?
            );
            Ok(())
        }
        Mode::ArtifactAttest { id, ev_type, note } => {
            if let Err(e) = browse_cli::ledger::validate_artifact_id(&id) {
                eprintln!("browse: {e}（用法错，退出 2）");
                std::process::exit(2);
            }
            if !ledger_client::ATTEST_TYPES.contains(&ev_type.as_str()) {
                eprintln!(
                    "browse: type 仅 {}（promote/demote/supersede 归 omc 工位），得 {ev_type}（用法错，退出 2）",
                    ledger_client::ATTEST_TYPES.join("|")
                );
                std::process::exit(2);
            }
            let r = ledger_call(move || {
                browse_cli::ledger::client()?
                    .artifact_attest(&id, &ev_type, json!({}), note.as_deref())
                    .map_err(|e| e.to_string())
            })
            .await?;
            println!("{}", serde_json::to_string_pretty(&r)?);
            Ok(())
        }
        Mode::ArtifactList { current, env_f } => {
            let r = ledger_call(move || {
                browse_cli::ledger::client()?
                    .artifact_list(current, env_f.as_deref())
                    .map_err(|e| e.to_string())
            })
            .await?;
            println!("{}", serde_json::to_string_pretty(&r)?);
            Ok(())
        }
        Mode::LedgerKeygen { force } => {
            let (kid, jwk, old) =
                browse_cli::ledger::keygen_write(force).map_err(|e| anyhow!("{e}"))?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "ok": true,
                    "keyId": kid,
                    "pubkeyJwk": jwk,
                    "replacedKid": old,
                    // 内置 JWK 的 kid 自证面（评审 G6）：与 keyId 对账——轮换后
                    // 须重烧常量再在册，两值不同即提示未烧
                    "builtinKid": browse_cli::ledger::key_id(),
                    "hint": "总台在册此 kid 后方可写入；私钥在 ~/.browse-rs/ledger/ed25519.key（0600），不打印不进 argv；keyId 与 builtinKid 不同 = 新钥未烧进常量"
                }))?
            );
            Ok(())
        }
        Mode::Fetch { url, timeout_s } => {
            let r = browse_cli::fetch::fetch(&url, true, timeout_s).await?;
            println!("{}", serde_json::to_string_pretty(&r)?);
            Ok(())
        }
        // ---- 片段库（#44）：纯文件系统读，零协议改动，不经 daemon ----
        Mode::SnippetsList(site) => {
            let root = browse_core::paths::state_dir().join("snippets");
            let mut found = 0usize;
            visit_snippets(&root, &root, site.as_deref(), &mut |rel, header| {
                found += 1;
                println!("{rel}  {header}");
            });
            if found == 0 {
                eprintln!(
                    "片段库为空（{}）；下一步：把可复用片段存成文件（首行 // 用途： 注释头），再 browse snippets list",
                    root.display()
                );
            }
            Ok(())
        }
        Mode::SnippetsShow(rel) => {
            if rel.is_empty() {
                // 用法错直出 exit 2（#53 评审 F1：G-F 口径补齐——snippets
                // show 从无 Mode 层缺参守卫，裸调曾落「片段 不存在」exit 1
                // 的误导面，与 workspace site/page 同款收口）
                eprintln!(
                    "browse: snippets show 缺 rel；用法：browse snippets show <rel>（退出 2）"
                );
                std::process::exit(2);
            }
            let root = browse_core::paths::state_dir().join("snippets");
            let path = root.join(&rel);
            // 越界守卫（#44 评审 F2）：canonicalize 后必须仍在库内，挡
            // 绝对路径、../ 与符号链接出库
            let Ok(canon) = path.canonicalize() else {
                bail!(
                    "片段 {rel} 不存在（{}）；下一步：browse snippets list 看在册片段",
                    path.display()
                );
            };
            let Ok(root_canon) = root.canonicalize() else {
                bail!(
                    "片段库目录不存在（{}）；下一步：browse snippets list 看在册片段",
                    root.display()
                );
            };
            if !canon.starts_with(&root_canon) {
                bail!(
                    "片段 {rel} 越出片段库（只许库内相对路径）；下一步：browse snippets list 看在册片段"
                );
            }
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    println!("{text}");
                    Ok(())
                }
                Err(_) => bail!(
                    "片段 {rel} 不存在（{}）；下一步：browse snippets list 看在册片段",
                    path.display()
                ),
            }
        }
        // ---- workspace 单仓（#50/#51 配套）：git 与文件系统操作，
        // 不经 daemon；blocking 全收 spawn_blocking（chrome_mgr 同构） ----
        Mode::WorkspaceStatus => {
            let root = browse_core::paths::workspace_dir();
            let st =
                tokio::task::spawn_blocking(move || browse_core::workspace::status_json(&root))
                    .await
                    .map_err(|e| anyhow!("workspace status 任务崩了：{e}"))??;
            if json {
                println!("{}", serde_json::to_string_pretty(&st)?);
            } else {
                print_workspace_status(&st);
            }
            Ok(())
        }
        Mode::WorkspaceInstall => {
            let root = browse_core::paths::workspace_dir();
            let r = tokio::task::spawn_blocking(move || browse_core::workspace::install(&root))
                .await
                .map_err(|e| anyhow!("workspace install 任务崩了：{e}"))??;
            println!("{}", serde_json::to_string_pretty(&r)?);
            Ok(())
        }
        Mode::WorkspaceUpdate => {
            let root = browse_core::paths::workspace_dir();
            let r = tokio::task::spawn_blocking(move || browse_core::workspace::update(&root))
                .await
                .map_err(|e| anyhow!("workspace update 任务崩了：{e}"))??;
            println!("{}", serde_json::to_string_pretty(&r)?);
            Ok(())
        }
        Mode::WorkspaceList => {
            let root = browse_core::paths::workspace_dir();
            let l = tokio::task::spawn_blocking({
                let root = root.clone();
                move || browse_core::workspace::list_json(&root)
            })
            .await
            .map_err(|e| anyhow!("workspace list 任务崩了：{e}"))?;
            let domains = l["domains"].as_array().cloned().unwrap_or_default();
            let pages = l["pages"].as_array().cloned().unwrap_or_default();
            if domains.is_empty() && pages.is_empty() {
                eprintln!(
                    "workspace 仓未安装或为空（{}）；下一步：browse workspace install",
                    root.display()
                );
                return Ok(());
            }
            println!("domain-skills:");
            for d in &domains {
                let seg = d["segment"].as_str().unwrap_or("?");
                let files = d["files"].as_array().cloned().unwrap_or_default();
                let names: Vec<&str> = files.iter().filter_map(|f| f.as_str()).collect();
                let capped = if d["capped"] == serde_json::json!(true) {
                    " …"
                } else {
                    ""
                };
                println!("  {seg}  {} 文件  {}{capped}", files.len(), names.join(" "));
            }
            println!("page-skills:");
            for p in &pages {
                println!("  {}", p.as_str().unwrap_or("?"));
            }
            Ok(())
        }
        Mode::WorkspaceSite(seg) => {
            if seg.is_empty() {
                // 用法错直出 exit 2（评审 G-F：口径与既有 bail_arg 族一致）
                eprintln!(
                    "browse: workspace site 缺段；用法：browse workspace site <段>[/<文件>]（退出 2）"
                );
                std::process::exit(2);
            }
            let root = browse_core::paths::workspace_dir();
            let text =
                tokio::task::spawn_blocking(move || browse_core::workspace::read_site(&root, &seg))
                    .await
                    .map_err(|e| anyhow!("workspace site 任务崩了：{e}"))??;
            println!("{text}");
            Ok(())
        }
        Mode::WorkspacePage(slug) => {
            if slug.is_empty() {
                eprintln!(
                    "browse: workspace page 缺 slug；用法：browse workspace page <slug>（退出 2）"
                );
                std::process::exit(2);
            }
            let root = browse_core::paths::workspace_dir();
            let text = tokio::task::spawn_blocking(move || {
                browse_core::workspace::read_page(&root, &slug)
            })
            .await
            .map_err(|e| anyhow!("workspace page 任务崩了：{e}"))??;
            println!("{text}");
            Ok(())
        }
        Mode::Eval => {
            // 裸调用 = 导航事件：无参不弹交互，TTY 裸跑出本仓帮助体 exit 0；
            // REPL 须 --repl 显式进；stdin 管道批处理形态不回归。
            if snippets.is_empty() {
                if repl {
                    if js {
                        eprintln!("browse: --js 与 --repl 不同行（REPL 是方言交互面，退出 2）");
                        std::process::exit(2);
                    }
                    return run_tty(new_tab, js).await;
                }
                if std::io::stdin().is_terminal() {
                    print_help();
                    return Ok(());
                }
                // 管道批处理不回归；空管道（EOF 无内容）视同裸调用出帮助体；
                // --js 空片段裸调用 = stdin 全量 JS 形态（#22）
                let n = run_stdin(new_tab, js, b64).await?;
                if n == 0 {
                    print_help();
                }
                return Ok(());
            }
            run_eval(
                snippets,
                new_tab,
                js,
                ws,
                port,
                chrome,
                headless,
                pipe,
                profile,
                proxy,
                proxy_bypass,
                isolated,
                secrets,
                engine_args,
            )
            .await
        }
    }
}

/// 用法错统一出口（退出 2）。各子命令旗标循环共用，措辞不点名子命令。
fn bail_arg(a: &str) -> ! {
    eprintln!("browse: 参数不认识 {a}（用法错，退出 2）");
    std::process::exit(2);
}

/// CLI 实参游标，必值与旗标收集两口分面。单口混用会互踩：必值守卫直出
/// exit 2（G-F 口径）后，靠「参数尽返 Err」收尾的旗标收集循环就永远等
/// 不到终点（0.12.1 子命令族假用法错回归的根因），故拆双口。
struct Args(std::iter::Skip<std::env::Args>);

impl Args {
    /// 取必值实参：参数尽即用法错直出 exit 2（G-F 口径，bail_arg 同族）。
    fn next(&mut self, flag: &str) -> String {
        match self.0.next() {
            Some(v) => v,
            None => {
                eprintln!("browse: {flag} 需要一个值（用法错，退出 2）");
                std::process::exit(2);
            }
        }
    }

    /// 收下一实参：参数尽返 None 即收尾。子命令旗标循环与可选位（如
    /// `snippets list [site]`）专用，不报错。
    fn next_opt(&mut self) -> Option<String> {
        self.0.next()
    }
}

fn mode_is_eval(m: &Mode) -> bool {
    matches!(m, Mode::Eval)
}

// 旗标原样透传求值前置的 engine_up，不做参数束重构（与 up 面同一套旗标族）
#[allow(clippy::too_many_arguments)]
async fn run_eval(
    snippets: Vec<String>,
    new_tab: bool,
    js: bool,
    ws: Option<String>,
    port: Option<u16>,
    chrome: Option<String>,
    headless: bool,
    pipe: bool,
    profile: Option<String>,
    proxy: Option<String>,
    proxy_bypass: Option<String>,
    isolated: bool,
    secrets: Option<String>,
    engine_args: Vec<String>,
) -> Result<()> {
    // --secrets 随新 daemon 环境注入（#25.4；已在跑 daemon 以启动时口径为准）；
    // CLI 进程自己也载一份（评审 F1）：return 出口的渲染发生在 CLI 侧，
    // 不载则展示面零脱敏——取值看 daemon 启动时仓，展示看本次 CLI 旗标
    if let Some(f) = secrets
        .clone()
        .or_else(|| std::env::var("BROWSE_SECRETS").ok())
    {
        if let Err(e) = browse_core::load_secrets(&f) {
            eprintln!("{e:#}");
            std::process::exit(1);
        }
        let mut envs: Vec<(&str, String)> = vec![("BROWSE_SECRETS", f)];
        envs.extend(client::skills_passthrough_env());
        client::ensure_daemon_with_env(&envs).await?;
    } else {
        client::ensure_daemon_with_env(&client::skills_passthrough_env()).await?;
    }
    // 显式连接意图先落引擎（up 面接受同样的旗标），再求值；proxy 三旗标
    // 同守卫透传（评审 F2：静默吞比报错更坑）
    if ws.is_some()
        || port.is_some()
        || chrome.is_some()
        || headless
        || pipe
        || profile.is_some()
        || proxy.is_some()
        || proxy_bypass.is_some()
        || isolated
        || !engine_args.is_empty()
    {
        client::engine_up(client::UpParams {
            headless,
            chrome,
            ws,
            port,
            pipe,
            profile,
            proxy,
            proxy_bypass,
            isolated,
            engine_args,
        })
        .await?;
    }
    // 空片段的裸调用分支已在 Mode::Eval 臂前置处理（帮助体 / 管道 / --repl），
    // 进到这里必带片段
    for snip in &snippets {
        run_snip(snip, new_tab, js).await;
    }
    Ok(())
}

async fn run_snip(snip: &str, new_tab: bool, js: bool) {
    match client::eval(snip, new_tab, js).await {
        Ok(v) => {
            // 大值落盘（artifact 降级形态）：stdout 只回路径与预览
            let s = match browse_cli::render::render_or_drop_sync(&v) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("browse: 大值落盘失败（{e}），改打全量前 1KB");
                    browse_core::render_result(&v)
                        .chars()
                        .take(1024)
                        .collect::<String>()
                }
            };
            if !s.is_empty() {
                println!("{s}");
            }
        }
        Err(e) => {
            // 错误链过密钥面具（#25.4 评审 G4 防御面）：daemon 侧已脱敏，
            // 这里兜 CLI 自加的 context 层（HTTP 失败嵌响应文本）
            eprintln!(
                "{}",
                browse_core::js_host::mask_secrets_str(&format!("{e:#}"))
            );
            std::process::exit(1);
        }
    }
}

async fn serve_foreground(
    bind: String,
    ws: Option<String>,
    port: Option<u16>,
    chrome: Option<std::path::PathBuf>,
    headless: bool,
    pipe: bool,
    profile: Option<String>,
) -> Result<()> {
    let session = cdp::Session::new();
    let host = browse_core::JsHost::new(session.clone());
    let engine = browse_core::Engine::new(session);
    let mut spec = if let Some(ws) = ws {
        browse_core::EngineSpec::Attach { ws_url: ws }
    } else if let Some(p) = port {
        browse_core::EngineSpec::Port(p)
    } else {
        browse_core::EngineSpec::from_env(chrome, headless, pipe)
    };
    // 显式 --profile 顶掉 from_env 的 BROWSE_PROFILE 缺省
    if let (browse_core::EngineSpec::Auto { profile: p, .. }, Some(explicit)) =
        (&mut spec, profile.map(std::path::PathBuf::from))
    {
        *p = Some(explicit);
    }
    // 密钥文件 daemon 启动即载（#25.4）：改密钥 = 重启 daemon（口径在册）
    if let Some(f) = std::env::var_os("BROWSE_SECRETS") {
        browse_core::load_secrets(&f.to_string_lossy())?;
    }
    let daemon = browse_core::server::Daemon::new(host, engine, spec);
    browse_core::server::serve(daemon, &bind).await
}

fn print_health(h: &serde_json::Value, json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(h).unwrap_or_default());
        return;
    }
    let engine = h.get("engine").cloned().unwrap_or(serde_json::Value::Null);
    let (kind, detail) = match engine.get("Spawned") {
        Some(s) => (
            "spawned",
            format!(
                "pid {} headless={} channel={} profile {}",
                s.get("pid").map(|v| v.to_string()).unwrap_or_default(),
                s.get("headless").and_then(|v| v.as_bool()).unwrap_or(false),
                s.get("channel").and_then(|v| v.as_str()).unwrap_or("port"),
                s.get("profile_dir").and_then(|v| v.as_str()).unwrap_or("")
            ),
        ),
        _ => match engine.get("Attached") {
            Some(a) => (
                "attached",
                a.get("ws_url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            ),
            _ => ("not-connected", String::new()),
        },
    };
    println!("daemon      http://{}", client::daemon_bind());
    println!(
        "instance    {}",
        h.get("name").and_then(|v| v.as_str()).unwrap_or("default")
    );
    println!(
        "uptime      {}s",
        h.get("uptime").and_then(|v| v.as_u64()).unwrap_or(0)
    );
    println!(
        "connected   {}",
        h.get("connected")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    );
    println!("engine      {kind} {detail}");
    println!(
        "active tab  {}",
        h.get("activeTargetId")
            .and_then(|v| v.as_str())
            .unwrap_or("-")
    );
}

async fn run_tty(new_tab: bool, js: bool) -> Result<()> {
    let mut rl = DefaultEditor::new()?;
    let mut buf = String::new();
    loop {
        let prompt = if buf.is_empty() { "js> " } else { "..> " };
        let line = match rl.readline(prompt) {
            Ok(l) => l,
            Err(ReadlineError::Interrupted) => {
                buf.clear();
                continue;
            }
            Err(ReadlineError::Eof) => break,
            Err(e) => return Err(e.into()),
        };
        if buf.is_empty() && matches!(line.trim(), "exit" | "quit" | ".exit") {
            break;
        }
        if line.trim() == "." {
            if !buf.trim().is_empty() {
                run_snip(&buf, new_tab, js).await;
            }
            buf.clear();
            continue;
        }
        if !buf.is_empty() {
            buf.push('\n');
        }
        buf.push_str(&line);
        if snippet_complete(&buf) {
            run_snip(&buf, new_tab, js).await;
            buf.clear();
        }
    }
    if !buf.trim().is_empty() {
        run_snip(&buf, new_tab, js).await;
    }
    Ok(())
}

/// 管道批处理；返回实际求值的段数（零段 = 空管道，裸调用面据此出帮助体）。
///
/// 形态（#22）：方言走括号配平分段；`--js` 走整段 stdin 一次求值（JS 的
/// 语句边界不由括号配平决定）；`--b64` 先整段 base64 解码再进各自形态
/// （`cat x.js.b64 | browse --js --b64` 即管道版全量 JS）。
async fn run_stdin(new_tab: bool, js: bool, b64: bool) -> Result<usize> {
    let mut raw = String::new();
    tokio::io::AsyncReadExt::read_to_string(&mut tokio::io::stdin(), &mut raw).await?;
    let text = if b64 {
        let t = raw.trim();
        if t.is_empty() {
            return Ok(0);
        }
        browse_cli::client::decode_arg_b64(t).unwrap_or_else(|e| {
            eprintln!("{e:#}");
            std::process::exit(2);
        })
    } else {
        raw
    };
    if js {
        // 全量 JS：整段一次求值（空段视同空管道，由裸调用面出帮助体）
        if text.trim().is_empty() {
            return Ok(0);
        }
        run_snip(&text, new_tab, js).await;
        return Ok(1);
    }
    // 行号口径注记（#33 G4）：本路径跳过空行攒 buf，报错行号相对实收
    // 片段（与 -e 直传的原始行号可能差前导空行数）
    let mut ran = 0usize;
    let mut buf = String::new();
    for line in text.lines() {
        let trimmed = line.trim_end_matches(['\r']);
        if trimmed.trim() == "." {
            if !buf.trim().is_empty() {
                run_snip(&buf, new_tab, js).await;
                ran += 1;
            }
            buf.clear();
            continue;
        }
        if !buf.is_empty() {
            buf.push('\n');
        }
        buf.push_str(trimmed);
        if snippet_complete(&buf) {
            run_snip(&buf, new_tab, js).await;
            ran += 1;
            buf.clear();
        }
    }
    if !buf.trim().is_empty() {
        run_snip(&buf, new_tab, js).await;
        ran += 1;
    }
    Ok(ran)
}

fn print_help() {
    // 帮助面由命令目录活树派生（surface::render_help，cli-docs 标准节序）；
    // --help、-h 与裸调用三入口共用本函数，节序与对齐的守卫在 surface_contract。
    println!("{}", browse_core::surface::render_help());
}

/// workspace status 的人读面（`--json` 走机器面直出）：何时用：
/// `browse workspace status` 未带 --json 时的渲染；安装态、git 概览
/// 与技能计数一行收束，未安装给 install 下一步，非 git 仓给托管
/// 面提示。边界：st 形不对时字段落 `?`/0 不 panic。
fn print_workspace_status(st: &serde_json::Value) {
    let root = st["root"].as_str().unwrap_or("?");
    if st["installed"] != serde_json::json!(true) {
        println!("workspace: 未安装（{root} 不存在）");
        if st["gitPresent"] != serde_json::json!(true) {
            println!("git: 未找到（下一步：装 git，或手工 git clone 种子仓到 {root}）");
        }
        println!("下一步：browse workspace install");
        return;
    }
    let remote = st["remote"].as_str();
    let branch = st["branch"].as_str().unwrap_or("?");
    let head = st["head"].as_str().unwrap_or("?");
    let dirty = st["dirtyChanges"].as_u64().unwrap_or(0);
    let dirty_s = if dirty > 0 {
        format!("，{dirty} 处未提交变更")
    } else {
        String::new()
    };
    // 非 git 仓（目录在但无 .git 或 origin 未登记）给可读提示（评审 G8）
    let remote_s = remote.unwrap_or("未登记（非 git 仓或无 origin）");
    println!("workspace: {root}（origin {remote_s}，{branch}@{head}{dirty_s}）");
    println!(
        "domain-skills: {} 站 {} 文件；page-skills: {} slug",
        st["domainSites"].as_u64().unwrap_or(0),
        st["domainFiles"].as_u64().unwrap_or(0),
        st["pageSlugs"].as_u64().unwrap_or(0),
    );
}

/// 递归走访片段库（#44）：目录形 `<site>/<task>.js` 天然分层；每文件取首行
/// `//` 注释头当摘要。只读，不建目录不写文件。
fn visit_snippets(
    root: &std::path::Path,
    dir: &std::path::Path,
    site: Option<&str>,
    out: &mut dyn FnMut(&str, &str),
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            // site 过滤：目录名前缀匹配
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if site.is_none_or(|want| name.contains(want)) {
                visit_snippets(root, &p, site, out);
            }
        } else if p
            .extension()
            .and_then(|x| x.to_str())
            .is_some_and(|x| x == "js" || x == "browse" || x == "txt")
        {
            let rel = p
                .strip_prefix(root)
                .ok()
                .and_then(|r| r.to_str())
                .unwrap_or_else(|| p.to_str().unwrap_or("?"));
            if let Some(site) = site
                && !rel.split('/').next().is_some_and(|seg| seg.contains(site))
                && !rel.contains(site)
            {
                continue;
            }
            let header = std::fs::read_to_string(&p)
                .ok()
                .and_then(|t| {
                    t.lines()
                        .find(|l| l.trim_start().starts_with("//"))
                        .map(|l| l.trim().trim_start_matches('/').trim().to_string())
                })
                .unwrap_or_default();
            out(rel, &header);
        }
    }
}
