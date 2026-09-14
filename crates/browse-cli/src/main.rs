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

use anyhow::{Result, anyhow};
use browse_cli::client;
use browse_core::snippet_complete;
use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;
use std::io::IsTerminal;
use tokio::io::{AsyncBufReadExt, BufReader};

/// 生命周期子命令。
enum Mode {
    /// 求值片段（缺省）。
    Eval,
    /// `browse up`：显式起引擎。
    Up,
    /// `browse down`：退 daemon（只杀自起引擎）。
    Down,
    /// `browse status`：看 daemon/引擎状态。
    Status,
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
    let mut headless = false;
    let mut pipe = false;
    let mut json = false;

    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut next = |flag: &str| -> Result<String> {
            args.next()
                .ok_or_else(|| anyhow!("browse: {flag} 需要一个值（用法错，退出 2）"))
        };
        match a.as_str() {
            "--eval" | "-e" => snippets.push(next("--eval")?),
            "--serve" => serve = true,
            "--bind" => bind = Some(next("--bind")?),
            "--connect" => connect = Some(next("--connect")?),
            "--new-tab" => new_tab = true,
            "--ws" => ws = Some(next("--ws")?),
            "--port" => {
                port = Some(
                    next("--port")?
                        .parse()
                        .map_err(|_| anyhow!("browse: --port 要数字（退出 2）"))?,
                )
            }
            "--chrome" => chrome = Some(next("--chrome")?),
            "--headless" => headless = true,
            "--pipe" => pipe = true,
            "--json" => json = true,
            "-h" | "--help" => {
                print_help();
                return Ok(());
            }
            "up" if snippets.is_empty() && mode_is_eval(&mode) => mode = Mode::Up,
            "down" if snippets.is_empty() && mode_is_eval(&mode) => mode = Mode::Down,
            "status" if snippets.is_empty() && mode_is_eval(&mode) => mode = Mode::Status,
            other if other.starts_with('-') => {
                eprintln!("browse: 未知参数 {other}（用法错，退出 2）");
                std::process::exit(2);
            }
            snippet => snippets.push(snippet.to_string()),
        }
    }

    if serve {
        let bind = bind.unwrap_or_else(client::daemon_bind);
        return serve_foreground(bind, ws, port, chrome.map(Into::into), headless, pipe).await;
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
            client::ensure_daemon().await?;
            let health = client::engine_up(headless, chrome, ws, port, pipe).await?;
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
        Mode::Status => {
            match client::daemon_alive().await {
                Some(h) => print_health(&h, json),
                None => println!("daemon 不在跑（跑任意片段或 browse up 自动拉起）"),
            }
            Ok(())
        }
        Mode::Eval => run_eval(snippets, new_tab, ws, port, chrome, headless, pipe).await,
    }
}

fn mode_is_eval(m: &Mode) -> bool {
    matches!(m, Mode::Eval)
}

async fn run_eval(
    snippets: Vec<String>,
    new_tab: bool,
    ws: Option<String>,
    port: Option<u16>,
    chrome: Option<String>,
    headless: bool,
    pipe: bool,
) -> Result<()> {
    client::ensure_daemon().await?;
    // 显式连接意图先落引擎（up 面接受同样的旗标），再求值
    if ws.is_some() || port.is_some() || chrome.is_some() || headless || pipe {
        client::engine_up(headless, chrome, ws, port, pipe).await?;
    }
    if !snippets.is_empty() {
        for snip in &snippets {
            run_snip(snip, new_tab).await;
        }
        return Ok(());
    }
    if std::io::stdin().is_terminal() {
        run_tty(new_tab).await
    } else {
        run_stdin(new_tab).await
    }
}

async fn run_snip(snip: &str, new_tab: bool) {
    match client::eval(snip, new_tab).await {
        Ok(v) => {
            let s = browse_core::render_result(&v);
            if !s.is_empty() {
                println!("{s}");
            }
        }
        Err(e) => {
            eprintln!("{e:#}");
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
) -> Result<()> {
    let session = cdp::Session::new();
    let host = browse_core::JsHost::new(session.clone());
    let engine = browse_core::Engine::new(session);
    let spec = if let Some(ws) = ws {
        browse_core::EngineSpec::Attach { ws_url: ws }
    } else if let Some(p) = port {
        browse_core::EngineSpec::Port(p)
    } else {
        browse_core::EngineSpec::from_env(chrome, headless, pipe)
    };
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

async fn run_tty(new_tab: bool) -> Result<()> {
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
                run_snip(&buf, new_tab).await;
            }
            buf.clear();
            continue;
        }
        if !buf.is_empty() {
            buf.push('\n');
        }
        buf.push_str(&line);
        if snippet_complete(&buf) {
            run_snip(&buf, new_tab).await;
            buf.clear();
        }
    }
    if !buf.trim().is_empty() {
        run_snip(&buf, new_tab).await;
    }
    Ok(())
}

async fn run_stdin(new_tab: bool) -> Result<()> {
    let mut stdin = BufReader::new(tokio::io::stdin());
    let mut line = String::new();
    let mut buf = String::new();
    loop {
        line.clear();
        let n = stdin.read_line(&mut line).await?;
        if n == 0 {
            break;
        }
        let trimmed = line.trim_end_matches(['\n', '\r']);
        if trimmed.trim() == "." {
            if !buf.trim().is_empty() {
                run_snip(&buf, new_tab).await;
            }
            buf.clear();
            continue;
        }
        if !buf.is_empty() {
            buf.push('\n');
        }
        buf.push_str(trimmed);
        if snippet_complete(&buf) {
            run_snip(&buf, new_tab).await;
            buf.clear();
        }
    }
    if !buf.trim().is_empty() {
        run_snip(&buf, new_tab).await;
    }
    Ok(())
}

fn print_help() {
    eprintln!(
        "\
browse — 给 agent 用的 browse CLI（clean-chrome 专属）

用法：
  browse '<方言片段>'                       求值（自动拉 daemon 与引擎）
  browse -e '<片段>' | stdin | TTY REPL      其余两形态
  browse --new-tab '<片段>'                  先开 about:blank 再求值
  browse --connect <ws|端口> '<片段>'        显式附着
  browse up [--headless] [--pipe] [--chrome <path>] [--ws <url>|--port <p>]   显式起引擎
  browse down                                退 daemon（只杀自起引擎）
  browse status [--json]                     状态
  browse --serve [--bind host:port]          前台跑 daemon

片段方言（与 browser-harness-js 对齐）：
  await session.connect({{port:9222}})
  const tabs = await listPageTargets()
  await session.use(tabs[0].targetId)
  await session.Page.navigate({{url:\"https://example.com\"}})
  await session.waitFor(\"Page.loadEventFired\", undefined, 15000)
支持：字面量/对象/数组/成员/下标/await/const-let-var/return。
不支持：函数字面量、if/for、模板字符串——页面逻辑放 Runtime.evaluate 的 expression。

环境：BROWSE_PORT（daemon 端口，默认 9880）、BROWSE_CHROME（chrome.exe 路径）、
      BROWSE_CDP_WS（钉死连接）、BROWSE_EVAL_TIMEOUT（秒，默认 300）。
      --pipe：spawn 引擎走 CDP 管道通道（CLEAN_CHROME_DEBUG=pipe，零 TCP 面）。
退出码：0 成功 / 1 执行失败 / 2 用法错。"
    );
}
