//! 命令面目录（incur-rs 原则的方言版适配）：CLI 子命令、方言全局函数、
//! session 方法**只在这一处登记为数据**，JSON Schema 与 LLM 清单
//! （`llms.txt` / `llms-full.txt`）全部由 [`render_llms`] 系列派生函数
//! 从本目录生成（`browse --gen-surface docs/surface` 重生成），agent 侧
//! 发现通道是 `browse --llms`（与本投影同源直出），`tests/surface_contract.rs`
//! 锁漂移。
//!
//! 与 incur（derive 宏命令图）的差异：我们的接口面是方言片段而非结构化
//! 参数，故目录手写为 `const`；incur 的技能（skill）物种不取，agent 说明书
//! 由 `browse --llms` 直出承担。

use serde_json::{Value, json};

/// 命令在目录中的种类：CLI 子命令、方言全局函数或 session 方法。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CmdKind {
    /// CLI 子命令或旗标形态。
    Cli,
    /// 方言宿主全局函数。
    Global,
    /// `session.<method>` 方法族。
    Session,
}

impl CmdKind {
    fn label(self) -> &'static str {
        match self {
            CmdKind::Cli => "CLI",
            CmdKind::Global => "全局函数",
            CmdKind::Session => "session 方法",
        }
    }
}

/// 描述一个参数的形状：名、类型、必填与否与缺省。
pub struct ArgSpec {
    /// 参数名。
    pub name: &'static str,
    /// `string` / `number` / `boolean` / `object` / `any`。
    pub ty: &'static str,
    /// 是否必填。
    pub required: bool,
    /// 缺省值描述（可省）。
    pub default: Option<&'static str>,
}

/// 命令目录里一条命令的登记项。
pub struct CmdSpec {
    /// 调用名（全局函数裸名；session 方法不带前缀）。
    pub name: &'static str,
    /// 种类。
    pub kind: CmdKind,
    /// 人读签名（含默认值提示）。
    pub signature: &'static str,
    /// 参数表。
    pub args: &'static [ArgSpec],
    /// 一句话说明。
    pub description: &'static str,
    /// 可照抄示例（方言片段或 shell 行）。
    pub example: &'static str,
}

macro_rules! arg {
    ($n:expr, $t:expr, $r:expr) => {
        ArgSpec {
            name: $n,
            ty: $t,
            required: $r,
            default: None,
        }
    };
    ($n:expr, $t:expr, $r:expr, $d:expr) => {
        ArgSpec {
            name: $n,
            ty: $t,
            required: $r,
            default: Some($d),
        }
    };
}

/// 全量命令目录，CLI/方言/session 三面的单一真相源。
pub const COMMANDS: &[CmdSpec] = &[
    // ---- CLI ----
    CmdSpec {
        name: "eval",
        kind: CmdKind::Cli,
        signature: "browse '<片段>'",
        args: &[arg!("snippet", "string", true)],
        description: "求值方言片段（缺省形态；自动拉起 daemon 与引擎）。",
        example: "browse 'return await currentTab()'",
    },
    CmdSpec {
        name: "eval-e",
        kind: CmdKind::Cli,
        signature: "browse -e '<片段>'",
        args: &[arg!("snippet", "string", true)],
        description: "显式 --eval 形态（与缺省等价）。",
        example: "browse -e 'return 1'",
    },
    CmdSpec {
        name: "eval-stdin",
        kind: CmdKind::Cli,
        signature: "browse < 文件",
        args: &[],
        description: "stdin 批处理：括号配平即提交一段，变量跨段共享。",
        example: "printf '%s\\n' 'const t = await listPageTargets()' 'return t[0].url' | browse",
    },
    CmdSpec {
        name: "new-tab-flag",
        kind: CmdKind::Cli,
        signature: "browse --new-tab '<片段>'",
        args: &[arg!("snippet", "string", true)],
        description: "求值前先开 about:blank 新 tab。",
        example: "browse --new-tab 'return await snapshot()'",
    },
    CmdSpec {
        name: "connect-flag",
        kind: CmdKind::Cli,
        signature: "browse --connect <ws|端口> '<片段>'",
        args: &[
            arg!("target", "string", true),
            arg!("snippet", "string", false),
        ],
        description: "显式附着给定 ws URL 或调试端口。",
        example: "browse --connect 9222 'return await listPageTargets()'",
    },
    CmdSpec {
        name: "up",
        kind: CmdKind::Cli,
        signature: "browse up [--headless] [--pipe] [--chrome <path>] [--profile <dir>] [--ws <url>] [--port <p>]",
        args: &[
            arg!("headless", "boolean", false, "false"),
            arg!("pipe", "boolean", false, "false"),
            arg!("chrome", "string", false, "发现序"),
            arg!("profile", "string", false, "固定 engine-profile"),
            arg!("ws", "string", false),
            arg!("port", "number", false),
        ],
        description: "显式起引擎（附着优先，缺则 spawn clean-chrome 隔离实例；--profile 自定义 user-data-dir，默认固定 profile 持久保存站点会话）。",
        example: "browse up --headless --profile ~/profiles/proj-a",
    },
    CmdSpec {
        name: "self-update",
        kind: CmdKind::Cli,
        signature: "browse update",
        args: &[],
        description: "自更新 browse 二进制（镜像 stable 段优先加 GitHub Releases 回退，sha256 锚校验，自证回滚；ark 管理安装拦走 ark）。",
        example: "browse update",
    },
    CmdSpec {
        name: "down",
        kind: CmdKind::Cli,
        signature: "browse down",
        args: &[],
        description: "退 daemon；只终结自己 spawn 的引擎，附着来源不动。",
        example: "browse down",
    },
    CmdSpec {
        name: "status",
        kind: CmdKind::Cli,
        signature: "browse status [--json]",
        args: &[arg!("json", "boolean", false, "false")],
        description: "daemon/引擎/实例名/活动 tab 概览。",
        example: "browse status --json",
    },
    CmdSpec {
        name: "llms-flag",
        kind: CmdKind::Cli,
        signature: "browse --llms [--full|--json]",
        args: &[
            arg!("full", "boolean", false, "false"),
            arg!("json", "boolean", false, "false"),
        ],
        description: "agent 手册直出（裸形 markdown 紧凑手册；--full 完整目录，--json 机器形 Schema；不拉 daemon）。",
        example: "browse --llms",
    },
    CmdSpec {
        name: "repl-flag",
        kind: CmdKind::Cli,
        signature: "browse --repl",
        args: &[],
        description: "显式进入交互 REPL（裸调用只出本仓帮助体，不弹交互）。",
        example: "browse --repl",
    },
    CmdSpec {
        name: "version-flag",
        kind: CmdKind::Cli,
        signature: "browse --version",
        args: &[],
        description: "打印版本号（资产解包冒烟与安装核对用）。",
        example: "browse --version",
    },
    CmdSpec {
        name: "chrome-install",
        kind: CmdKind::Cli,
        signature: "browse chrome install <版本> [部署目录]",
        args: &[
            arg!("version", "string", true),
            arg!("fromDir", "string", false, "缺省走 R2 镜像下载"),
        ],
        description: "安装 Chromium 版本并 pin：缺省从镜像下载（版本段路由加 .sha256 锚校验原子落位），给部署目录则本地导入。",
        example: "browse chrome install 152.0.7977.84",
    },
    CmdSpec {
        name: "chrome-list",
        kind: CmdKind::Cli,
        signature: "browse chrome list",
        args: &[],
        description: "列已装 Chromium 版本与当前 pin。",
        example: "browse chrome list",
    },
    CmdSpec {
        name: "chrome-use",
        kind: CmdKind::Cli,
        signature: "browse chrome use <版本>",
        args: &[arg!("version", "string", true)],
        description: "把托管引擎 pin 切到已装版本（旧版保留可回退）。",
        example: "browse chrome use 152.0.7977.84",
    },
    CmdSpec {
        name: "chrome-update",
        kind: CmdKind::Cli,
        signature: "browse chrome update",
        args: &[],
        description: "升级到镜像最新版并默认切用（发现源 latest，BROWSE_CHROME_LATEST 可钉）。",
        example: "browse chrome update",
    },
    CmdSpec {
        name: "chrome-remove",
        kind: CmdKind::Cli,
        signature: "browse chrome remove <版本>",
        args: &[arg!("version", "string", true)],
        description: "删已装版本（目录加登记一起清；pin 指向的拒删，先 use 切走）。",
        example: "browse chrome remove 152.0.7977.84",
    },
    CmdSpec {
        name: "chrome-doctor",
        kind: CmdKind::Cli,
        signature: "browse chrome doctor",
        args: &[],
        description: "托管 Chromium 部署体检（在位/文件基线/pin）。",
        example: "browse chrome doctor",
    },
    CmdSpec {
        name: "issue-new",
        kind: CmdKind::Cli,
        signature: "browse issue new <标题> [--body <正文>]",
        args: &[
            arg!("title", "string", true),
            arg!("body", "string", false, "旗标缺省吃管道 stdin"),
        ],
        description: "一键缺陷反馈：自动署名 tool=browse 加版本加平台加主机。",
        example: "browse issue new <标题> --body <复现步骤>",
    },
    CmdSpec {
        name: "issue-list",
        kind: CmdKind::Cli,
        signature: "browse issue list [--status <s>] [--limit <n>] [--tool <t>]",
        args: &[
            arg!("status", "string", false),
            arg!("limit", "number", false, "20"),
            arg!("tool", "string", false, "browse"),
        ],
        description: "列 issue（新到旧；默认本工具，--tool 换过滤）。",
        example: "browse issue list --limit 10",
    },
    CmdSpec {
        name: "issue-show",
        kind: CmdKind::Cli,
        signature: "browse issue show <id>",
        args: &[arg!("id", "string", true)],
        description: "看 issue 详情。",
        example: "browse issue show 42",
    },
    CmdSpec {
        name: "serve",
        kind: CmdKind::Cli,
        signature: "browse --serve [--bind host:port]",
        args: &[arg!("bind", "string", false, "127.0.0.1:9880")],
        description: "前台跑 daemon（--bind 选监听地址）。",
        example: "browse --serve --bind 127.0.0.1:9880",
    },
    // ---- 全局函数 ----
    CmdSpec {
        name: "listPageTargets",
        kind: CmdKind::Global,
        signature: "listPageTargets()",
        args: &[],
        description: "列可附着 page targets（带 own 标：只有 own=true 可 closeTab）。",
        example: "const tabs = await listPageTargets()",
    },
    CmdSpec {
        name: "resolveWsUrl",
        kind: CmdKind::Global,
        signature: "resolveWsUrl(opts?)",
        args: &[arg!("opts", "object", false, "{port:9222}")],
        description: "把 wsUrl/port/profileDir 线索解析成 WS URL（不连接）。",
        example: "return await resolveWsUrl({port: 9222})",
    },
    CmdSpec {
        name: "detectBrowsers",
        kind: CmdKind::Global,
        signature: "detectBrowsers()",
        args: &[],
        description: "探测本机已开的调试浏览器（profile/端口/wsUrl）。",
        example: "return await detectBrowsers()",
    },
    CmdSpec {
        name: "chromeInstall",
        kind: CmdKind::Global,
        signature: "chromeInstall(opts?)",
        args: &[arg!("opts", "object", false, "{fromDir}|{version}")],
        description: "安装 Chromium 版本并 pin：fromDir 本地导入（version 缺省取目录名），version 单给走 R2 镜像下载（sha256 锚校验后原子落位）。",
        example: "await chromeInstall({version: \"152.0.7977.84\"})",
    },
    CmdSpec {
        name: "chromeList",
        kind: CmdKind::Global,
        signature: "chromeList()",
        args: &[],
        description: "列已装 Chromium 版本与当前 pin。",
        example: "return await chromeList()",
    },
    CmdSpec {
        name: "chromeUse",
        kind: CmdKind::Global,
        signature: "chromeUse(version)",
        args: &[arg!("version", "string", true)],
        description: "pin 切到已装版本（引擎发现序的托管位）。",
        example: "await chromeUse(\"152.0.7977.84\")",
    },
    CmdSpec {
        name: "chromeUpdate",
        kind: CmdKind::Global,
        signature: "chromeUpdate()",
        args: &[],
        description: "升级到镜像最新版并 pin 切过去（发现源 latest，BROWSE_CHROME_LATEST 可钉）。",
        example: "return await chromeUpdate()",
    },
    CmdSpec {
        name: "chromeRemove",
        kind: CmdKind::Global,
        signature: "chromeRemove(version)",
        args: &[arg!("version", "string", true)],
        description: "删已装版本（目录加登记一起清；pin 指向的拒删，先 chromeUse 切走）。",
        example: "await chromeRemove(\"152.0.7977.84\")",
    },
    CmdSpec {
        name: "chromeDoctor",
        kind: CmdKind::Global,
        signature: "chromeDoctor()",
        args: &[],
        description: "托管 Chromium 部署体检（在位/文件基线/pin 健康）。",
        example: "return await chromeDoctor()",
    },
    CmdSpec {
        name: "cdpMethods",
        kind: CmdKind::Global,
        signature: "cdpMethods(domain?)",
        args: &[arg!("domain", "string", false, "全部")],
        description: "652 个 CDP 命令的运行时探针（按域过滤）。",
        example: "return await cdpMethods(\"Network\")",
    },
    CmdSpec {
        name: "snapshot",
        kind: CmdKind::Global,
        signature: "snapshot()",
        args: &[],
        description: "AX 树快照：nodes 带 role/name/value/短 ref（e1、e2…），引用表的唯一来源。",
        example: "const s = await snapshot()",
    },
    CmdSpec {
        name: "screenshot",
        kind: CmdKind::Global,
        signature: "screenshot(path?, full?)",
        args: &[
            arg!("path", "string", false, "<state>/screenshots/…png"),
            arg!("full", "boolean", false, "false"),
        ],
        description: "页内截图存 PNG，回 {path,bytes}。",
        example: "return await screenshot()",
    },
    CmdSpec {
        name: "pdf",
        kind: CmdKind::Global,
        signature: "pdf(path?)",
        args: &[arg!("path", "string", false, "<state>/pdfs/…pdf")],
        description: "当前页存 PDF（仅无头 chrome），回 {path,bytes}。",
        example: "return await pdf()",
    },
    CmdSpec {
        name: "newTab",
        kind: CmdKind::Global,
        signature: "newTab(url?)",
        args: &[arg!("url", "string", false, "about:blank")],
        description: "开新 tab 并设为活动路由（先 about:blank 再 goto，防竞速假完成）。",
        example: "await newTab(\"https://example.com\")",
    },
    CmdSpec {
        name: "switchTab",
        kind: CmdKind::Global,
        signature: "switchTab(targetId)",
        args: &[arg!("targetId", "string", true)],
        description: "切活动路由（不改 Chrome 可见前景，人机共存）。",
        example: "await switchTab(tabs[0].targetId)",
    },
    CmdSpec {
        name: "currentTab",
        kind: CmdKind::Global,
        signature: "currentTab()",
        args: &[],
        description: "当前活动 tab 简表（无活动返回 null）。",
        example: "return await currentTab()",
    },
    CmdSpec {
        name: "closeTab",
        kind: CmdKind::Global,
        signature: "closeTab(targetId?)",
        args: &[arg!("targetId", "string", false, "当前活动 tab")],
        description: "关 tab；守卫只放行自建 tab（chrome 初始页与用户 tab 拒绝）。",
        example: "await closeTab((await currentTab()).targetId)",
    },
    CmdSpec {
        name: "clickAt",
        kind: CmdKind::Global,
        signature: "clickAt(x, y)",
        args: &[arg!("x", "number", true), arg!("y", "number", true)],
        description: "视口坐标 trusted 点击（点当前可见物，不做遮挡检查）。",
        example: "await clickAt(120, 40)",
    },
    CmdSpec {
        name: "fillInput",
        kind: CmdKind::Global,
        signature: "fillInput(selector, text)",
        args: &[
            arg!("selector", "string", true),
            arg!("text", "string", true),
        ],
        description: "按 CSS 选择器填输入框（SelectAll 不发 Ctrl+A，回读严格验证；select 走 selectOption）。",
        example: "await fillInput(\"#q\", \"hello\")",
    },
    CmdSpec {
        name: "clickRef",
        kind: CmdKind::Global,
        signature: "clickRef(ref)",
        args: &[arg!("ref", "string", true)],
        description: "按 snapshot 短 ref 点击：滚动可见、量中心、遮挡命中测试（被盖即拒绝并报遮挡物）、trusted 派发。",
        example: "await clickRef(\"e3\")",
    },
    CmdSpec {
        name: "fillRef",
        kind: CmdKind::Global,
        signature: "fillRef(ref, text)",
        args: &[arg!("ref", "string", true), arg!("text", "string", true)],
        description: "按 ref 填输入框：objectId focus、SelectAll+insertText、同节点回读严格验证。",
        example: "await fillRef(\"e2\", \"hello\")",
    },
    CmdSpec {
        name: "selectOption",
        kind: CmdKind::Global,
        signature: "selectOption(ref, value)",
        args: &[arg!("ref", "string", true), arg!("value", "string", true)],
        description: "下拉框选择（value 或可见 label；设值+派发 input/change；未命中报全部可选值）。",
        example: "return await selectOption(\"e4\", \"Beta\")",
    },
    CmdSpec {
        name: "pressKey",
        kind: CmdKind::Global,
        signature: "pressKey(key)",
        args: &[arg!("key", "string", true)],
        description: "按一个键（Enter/Tab/单字符；Enter 的 text 是 \\r，CDP 契约）。",
        example: "await pressKey(\"Enter\")",
    },
    CmdSpec {
        name: "dialogStatus",
        kind: CmdKind::Global,
        signature: "dialogStatus()",
        args: &[],
        description: "当前 JS 对话框状态（open/type/message/defaultPrompt）。",
        example: "return await dialogStatus()",
    },
    CmdSpec {
        name: "dialogAccept",
        kind: CmdKind::Global,
        signature: "dialogAccept(text?)",
        args: &[arg!("text", "string", false)],
        description: "接受当前对话框（prompt 可带应答文本）。",
        example: "await dialogAccept(\"yes\")",
    },
    CmdSpec {
        name: "dialogDismiss",
        kind: CmdKind::Global,
        signature: "dialogDismiss()",
        args: &[],
        description: "拒绝当前对话框。alert/beforeunload 会被自动接受，无需手动。",
        example: "await dialogDismiss()",
    },
    CmdSpec {
        name: "routeBlock",
        kind: CmdKind::Global,
        signature: "routeBlock(pattern)",
        args: &[arg!("pattern", "string", true)],
        description: "网络拦截：glob 命中的请求直接失败（BlockedByClient）。作用于当前活动 tab。",
        example: "await routeBlock(\"*://ads.example.com/*\")",
    },
    CmdSpec {
        name: "routeMock",
        kind: CmdKind::Global,
        signature: "routeMock(pattern, body, opts?)",
        args: &[
            arg!("pattern", "string", true),
            arg!("body", "string", true),
            arg!(
                "opts",
                "object",
                false,
                "{status:200, contentType:\"text/html\"}"
            ),
        ],
        description: "网络拦截：命中的请求本地应答（默认带 ACAO *）。",
        example: "await routeMock(\"http://mock.test/api*\", \"{\\\"ok\\\":1}\", {contentType: \"application/json\"})",
    },
    CmdSpec {
        name: "routeClear",
        kind: CmdKind::Global,
        signature: "routeClear()",
        args: &[],
        description: "清空全部拦截规则并 Fetch.disable。",
        example: "await routeClear()",
    },
    CmdSpec {
        name: "waitLoad",
        kind: CmdKind::Global,
        signature: "waitLoad(ms?)",
        args: &[arg!("ms", "number", false, "10000")],
        description: "等 document.readyState 到 complete（已加载立即返回）。",
        example: "await waitLoad(8000)",
    },
    CmdSpec {
        name: "waitForResponse",
        kind: CmdKind::Global,
        signature: "waitForResponse(pattern, ms?)",
        args: &[arg!("pattern", "string", true), arg!("ms", "int", false)],
        description: "等 URL 命中 glob（与 routeBlock/routeMock 同写法）的最近一个响应完成（#20，只认活动 tab）：回 {requestId,url,status,headers,body,base64Encoded,json}；命中含历史（Network 域须在触发前已开，本函数幂等开收不到已发出的响应），方言无并发，可用形态是触发后等待；体等 loadingFinished 再取（5 秒窗），失败显式 bodyError；base64 自动解码，可解析时附 json。",
        example: r#"return await waitForResponse("https://x.test/api*")"#,
    },
    CmdSpec {
        name: "responseBody",
        kind: CmdKind::Global,
        signature: "responseBody(requestId)",
        args: &[arg!("requestId", "string", true)],
        description: "按 requestId 取响应体（#20）：peekEvents/findEvents 拿到的 id 皆可用，base64 自动解码，回 {body,base64Encoded,json} 与 waitForResponse 同形。",
        example: r#"return await responseBody(rid)"#,
    },
    CmdSpec {
        name: "waitIdle",
        kind: CmdKind::Global,
        signature: "waitIdle(ms?)",
        args: &[arg!("ms", "number", false, "10000")],
        description: "等 network 静默（窗口语义：起点前挂着的请求不计）。",
        example: "await waitIdle(5000)",
    },
    CmdSpec {
        name: "recordStart",
        kind: CmdKind::Global,
        signature: "recordStart(opts?)",
        args: &[arg!("opts", "object", false, "{everyNthFrame:1}")],
        description: "开始录屏（Screencast 帧流落盘；everyNthFrame 源端抽帧、maxWidth/maxHeight 限宽高）。",
        example: "return await recordStart({everyNthFrame: 2})",
    },
    CmdSpec {
        name: "recordStop",
        kind: CmdKind::Global,
        signature: "recordStop()",
        args: &[],
        description: "停止录制，回 {frames,bytes,dir,sessionChanged}（PNG 已在盘上；录制中被换靶则 true，帧流钉住不中断）。",
        example: "return await recordStop()",
    },
    CmdSpec {
        name: "hostFunctions",
        kind: CmdKind::Global,
        signature: "hostFunctions()",
        args: &[],
        description: "本目录的运行时探针（agent 自描述，schema/清单同源）。",
        example: "return await hostFunctions()",
    },
    CmdSpec {
        name: "print",
        kind: CmdKind::Global,
        signature: "print(x)",
        args: &[arg!("x", "any", true)],
        description: "把值打到 daemon stderr（调试用）。",
        example: "await print(tabs)",
    },
    CmdSpec {
        name: "JSON.parse",
        kind: CmdKind::Global,
        signature: "JSON.parse(s)",
        args: &[arg!("s", "string", true)],
        description: "宿主侧把 JSON 文本解析成值（getResponseBody 等结果的小加工，#21）。",
        example: r#"return JSON.parse(body).title"#,
    },
    CmdSpec {
        name: "JSON.stringify",
        kind: CmdKind::Global,
        signature: "JSON.stringify(v, indent?)",
        args: &[arg!("v", "any", true), arg!("indent", "int", false)],
        description: "宿主侧把值序列化成字符串回传（indent 大于 0 出两格缩进多行形）。",
        example: "return JSON.stringify(tabs, 2)",
    },
    CmdSpec {
        name: "value-methods",
        kind: CmdKind::Global,
        signature: "值.slice(start,end?) 等",
        args: &[],
        description: "字符串与数组的小加工（#21）：字符串 slice(start,end?)/split(sep)/includes(sub)/startsWith(sub)/endsWith(sub)/trim()/toUpperCase()/toLowerCase()；数组 slice(start,end?)/join(sep?)/includes(v)/concat(数组...)。slice 按 UTF-16 单元（同 .length，代理对切中间出替换符）；大小写转换非 locale；join 对容器元素打紧凑 JSON；includes 数值宽等（1 与 1.0 同值），容器按 JSON 深等（与 JS 引用等值不同）。纯函数无控制流，页面内逻辑仍走 Runtime.evaluate。",
        example: r#"return body.slice(0, 200)"#,
    },
    // ---- session 方法 ----
    CmdSpec {
        name: "connect",
        kind: CmdKind::Session,
        signature: "session.connect(opts?)",
        args: &[arg!("opts", "object", false, "{port:9222, timeoutMs:5000}")],
        description: "按 wsUrl/port/profileDir 连接（timeoutMs 可等人工 Allow）。",
        example: "await session.connect({port: 9222, timeoutMs: 30000})",
    },
    CmdSpec {
        name: "use",
        kind: CmdKind::Session,
        signature: "session.use(targetId)",
        args: &[arg!("targetId", "string", true)],
        description: "附着某 tab 并设为活动路由（flatten attach）。",
        example: "await session.use(tabs[0].targetId)",
    },
    CmdSpec {
        name: "close",
        kind: CmdKind::Session,
        signature: "session.close()",
        args: &[],
        description: "断开连接不关浏览器（可重连；spawn 端口态引擎会被引擎策略重连）。",
        example: "await session.close()",
    },
    CmdSpec {
        name: "setActiveSession",
        kind: CmdKind::Session,
        signature: "session.setActiveSession(sessionId?)",
        args: &[arg!("sessionId", "string", false)],
        description: "覆写活动 sessionId（高级用法；一般走 session.use）。",
        example: "await session.setActiveSession(sid)",
    },
    CmdSpec {
        name: "getActiveSession",
        kind: CmdKind::Session,
        signature: "session.getActiveSession()",
        args: &[],
        description: "当前活动 sessionId（无则 null）。",
        example: "return await session.getActiveSession()",
    },
    CmdSpec {
        name: "isConnected",
        kind: CmdKind::Session,
        signature: "session.isConnected()",
        args: &[],
        description: "连接是否存活。",
        example: "return await session.isConnected()",
    },
    CmdSpec {
        name: "waitFor",
        kind: CmdKind::Session,
        signature: "session.waitFor(method, undefined, ms?)",
        args: &[
            arg!("method", "string", true),
            arg!("ms", "number", false, "15000"),
        ],
        description: "从事件缓冲取第一个 method 事件（取出即移除；超时报错）。",
        example: "await session.waitFor(\"Page.loadEventFired\", undefined, 15000)",
    },
    CmdSpec {
        name: "waitJs",
        kind: CmdKind::Session,
        signature: "session.waitJs(expression, ms?)",
        args: &[
            arg!("expression", "string", true),
            arg!("ms", "number", false, "10000"),
        ],
        description: "页内谓词轮询（真 V8 表达式），等到即返回真值本身。",
        example: "await session.waitJs(\"document.querySelector('#x') !== null\", 5000)",
    },
    CmdSpec {
        name: "call",
        kind: CmdKind::Session,
        signature: "session.call(method, params?)",
        args: &[
            arg!("method", "string", true),
            arg!("params", "object", false, "{}"),
        ],
        description: "裸调任意 CDP 方法（守卫与 sessionId 路由同 session.<Domain>.<method>）。",
        example: "await session.call(\"Page.navigate\", {url: \"https://example.com\"})",
    },
    CmdSpec {
        name: "peekEvents",
        kind: CmdKind::Session,
        signature: "session.peekEvents(method, n?)",
        args: &[
            arg!("method", "string", true),
            arg!("n", "number", false, "1"),
        ],
        description: "非破坏窥视事件缓冲（waitFor 之后仍能取到）。",
        example: "return await session.peekEvents(\"Network.requestWillBeSent\", 5)",
    },
    CmdSpec {
        name: "peekEventsSince",
        kind: CmdKind::Session,
        signature: "session.peekEventsSince(method, sinceSeq, n?)",
        args: &[
            arg!("method", "string", true),
            arg!("sinceSeq", "number", true),
            arg!("n", "number", false, "1"),
        ],
        description: "seq 游标增量窥视（首拍 sinceSeq 用 0，之后用上一拍最后一条的 seq）。",
        example: "return await session.peekEventsSince(\"Network.requestWillBeSent\", 42, 10)",
    },
    CmdSpec {
        name: "findEvents",
        kind: CmdKind::Session,
        signature: "session.findEvents(method, path, value, n?)",
        args: &[
            arg!("method", "string", true),
            arg!("path", "string", true),
            arg!("value", "any", true),
            arg!("n", "number", false, "1"),
        ],
        description: "等值过滤窥视（点分路径，如 params.requestId）。",
        example: "return await session.findEvents(\"Network.responseReceived\", \"params.response.status\", 404, 1)",
    },
];

/// 返回目录的运行时 JSON，即 hostFunctions 探针的返回体。
pub fn catalog_json() -> Value {
    Value::Array(
        COMMANDS
            .iter()
            .map(|c| {
                json!({
                    "name": c.name,
                    "kind": c.kind.label(),
                    "signature": c.signature,
                    "description": c.description,
                    "example": c.example,
                    "args": c.args.iter().map(|a| json!({
                        "name": a.name,
                        "type": a.ty,
                        "required": a.required,
                        "default": a.default,
                    })).collect::<Vec<_>>(),
                })
            })
            .collect(),
    )
}

fn input_schema(c: &CmdSpec) -> Value {
    let mut props = serde_json::Map::new();
    let mut required = Vec::new();
    for a in c.args {
        props.insert(
            a.name.to_string(),
            json!({
                "type": a.ty,
                "description": if let Some(d) = a.default {
                    format!("缺省 {d}")
                } else {
                    String::new()
                },
            }),
        );
        if a.required {
            required.push(json!(a.name));
        }
    }
    json!({
        "type": "object",
        "properties": props,
        "required": required,
    })
}

fn def_name(c: &CmdSpec) -> String {
    match c.kind {
        CmdKind::Session => format!("session.{name}", name = c.name),
        CmdKind::Cli => format!("cli.{name}", name = c.name),
        CmdKind::Global => c.name.to_string(),
    }
}

/// 渲染 JSON Schema 包：每命令一个 definition，输入按目录、输出是运行时 JSON。
pub fn render_schema() -> Value {
    let mut defs = serde_json::Map::new();
    for c in COMMANDS {
        defs.insert(
            def_name(c),
            json!({
                "type": "object",
                "description": format!("{} {}", c.signature, c.description),
                "input": input_schema(c),
                "output": "runtime JSON（回值随页面与 CDP 而定）",
                "example": c.example,
            }),
        );
    }
    json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "title": "browse-rs 命令面",
        "description": "由 crates/browse-core/src/surface.rs 目录派生；browse --gen-surface 重生成，tests/surface_contract.rs 锁漂移",
        "definitions": defs,
    })
}

/// 渲染 `--llms` agent 手册（REQ-060 一面，族标准名）：名加版本加一句定位加子命令表
/// 加通用旗标加常用例。全量由 [`COMMANDS`] 活树派生，禁手维护双份；
/// 行数帽 120 由 `manual_under_120_lines` 测试锁（目录膨胀时逼收敛）。
pub fn render_manual() -> String {
    let mut out = String::from("# browse\n\n");
    out.push_str(&format!("版本 {}。\n", env!("CARGO_PKG_VERSION")));
    out.push_str(
        "定位：给 agent（也给人）的浏览器驾驶 CLI；JS 方言片段驱动 clean-chrome，\
         常驻 daemon 会话跨命令存活。\n\n",
    );
    out.push_str(
        "## 读序\n\n先看「子命令」表选形态：页面操作走方言片段（缺省形态），\
         引擎生命周期走 up/down/status，Chromium 版本走 chrome 子命令，\
         缺陷反馈走 issue 子命令。错误回执自带可照抄「下一步」。\n\n",
    );
    out.push_str("## 子命令\n\n| 形态 | 说明 |\n| --- | --- |\n");
    for c in COMMANDS
        .iter()
        .filter(|c| c.kind == CmdKind::Cli && !c.signature.starts_with("browse --"))
    {
        out.push_str(&format!("| `{}` | {} |\n", c.signature, c.description));
    }
    out.push_str("\n## 通用旗标\n\n| 旗标 | 说明 |\n| --- | --- |\n");
    for c in COMMANDS
        .iter()
        .filter(|c| c.kind == CmdKind::Cli && c.signature.starts_with("browse --"))
    {
        out.push_str(&format!("| `{}` | {} |\n", c.signature, c.description));
    }
    out.push_str(
        "\n## 退出码\n\n| 码 | 义 |\n| --- | --- |\n| 0 | 成功 |\n\
         | 1 | 执行失败 |\n| 2 | 用法错 |\n",
    );
    out.push_str("\n## 常用例\n\n```bash\n");
    for c in COMMANDS.iter().filter(|c| c.kind == CmdKind::Cli) {
        out.push_str(&format!("{}\n", c.example));
    }
    out.push_str("```\n");
    out
}

// Options 节的伴生旗标：不单独成目录条目（up 与求值前置共用），与目录旗标
// 合并按字典序渲染；描述与 [`COMMANDS`] 条目无重复。
const COMPANION_FLAGS: &[(&str, &str)] = &[
    (
        "--bind <host:port>",
        "--serve 监听地址（default: 127.0.0.1:9880）",
    ),
    (
        "--chrome <path>",
        "显式引擎路径（伴 up 与求值前置；缺省走发现序）",
    ),
    ("--eval, -e <片段>", "显式求值（与缺省形态等价）"),
    ("--full", "--llms 变体：完整目录"),
    ("--headless", "无头引擎（伴 up 与求值前置）"),
    ("--help, -h", "人读帮助"),
    (
        "--json",
        "JSON 输出（伴 status；--llms --json 出机器形 Schema）",
    ),
    ("--pipe", "spawn 引擎走 CDP 管道（零 TCP 面）"),
    ("--port <p>", "显式调试端口（伴附着与 up）"),
    (
        "--profile <dir>",
        "自定义 user-data-dir（default: 固定 engine-profile）",
    ),
    ("--ws <url>", "显式 WS URL（伴附着与 up）"),
];

// 展示宽度：CJK 与全角区记 2，其余记 1（Commands/Options 列对齐用）。
fn disp_width(s: &str) -> usize {
    s.chars()
        .map(|c| {
            if ('\u{3000}'..='\u{9fff}').contains(&c) || ('\u{ff00}'..='\u{ffef}').contains(&c) {
                2
            } else {
                1
            }
        })
        .sum()
}

/// 渲染 `--help` 与裸调用共用的帮助面（cli-docs 标准节序）：头行 name@版本
/// 加一句定位、Usage synopsis、Commands（[`COMMANDS`] 活树派生，描述单一
/// 真源）、Options（目录旗标加伴生旗标，字典序列对齐）、片段方言、环境
/// 变量、退出码。`help_covers_catalog` 守卫测试锁命令树全覆盖与版本注入。
///
/// ```
/// let help = browse_core::surface::render_help();
/// assert!(help.contains("Usage:"));
/// assert!(help.contains(&format!("browse@{}", env!("CARGO_PKG_VERSION"))));
/// assert!(help.contains("Commands:"));
/// ```
pub fn render_help() -> String {
    let mut out = format!(
        "browse@{} 给 agent（也给人）的浏览器驾驶 CLI\n\n",
        env!("CARGO_PKG_VERSION")
    );
    out.push_str("Usage: browse [options] '<方言片段>'\n       browse <command> [options]\n\n");

    // Commands：目录 CLI 形态条目（子命令与缺省形态），名截去可选尾按最长名对齐
    let cmds: Vec<(&str, &str)> = COMMANDS
        .iter()
        .filter(|c| c.kind == CmdKind::Cli && !c.signature.starts_with("browse --"))
        .map(|c| {
            (
                c.signature.split(" [").next().unwrap_or(c.signature),
                c.description.trim_end_matches('。'),
            )
        })
        .collect();
    let cw = cmds.iter().map(|(n, _)| disp_width(n)).max().unwrap_or(0);
    out.push_str("Commands:\n");
    for (n, d) in &cmds {
        out.push_str(&format!("  {n}{}  {d}\n", " ".repeat(cw - disp_width(n))));
    }

    // Options：目录旗标条目（描述单一真源）加伴生旗标，合并字典序。
    // 旗标名取签名里旗标 token 连取值占位（遇可选 [ 与片段实参 '< 截止）。
    let mut flags: Vec<(String, &str)> = COMMANDS
        .iter()
        .filter(|c| c.kind == CmdKind::Cli && c.signature.starts_with("browse --"))
        .map(|c| {
            let name = c
                .signature
                .split(' ')
                .skip(1)
                .take_while(|t| !t.starts_with('[') && !t.starts_with('\''))
                .collect::<Vec<_>>()
                .join(" ");
            (name, c.description.trim_end_matches('。'))
        })
        .collect();
    flags.extend(COMPANION_FLAGS.iter().map(|(n, d)| ((*n).to_string(), *d)));
    flags.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    let fw = flags.iter().map(|(n, _)| disp_width(n)).max().unwrap_or(0);
    out.push_str("\nOptions:\n");
    for (n, d) in &flags {
        out.push_str(&format!("  {n}{}  {d}\n", " ".repeat(fw - disp_width(n))));
    }

    out.push_str(
        "\n片段方言：\n  await session.connect({port:9222})\n  const tabs = await listPageTargets()\n  await session.use(tabs[0].targetId)\n  await session.Page.navigate({url:\"https://example.com\"})\n  await session.waitFor(\"Page.loadEventFired\", undefined, 15000)\n  支持：字面量/对象/数组/成员/下标/await/const-let-var/return。\n  模板字符串（反引号）raw 语义：内容逐字保留（只有 \\\\` 与 \\\\${ 是转义，模板内 \\\\${ 降格为 ${），可多行，页面代码原样内嵌 expression。\n  普通字符串只转义 \\\\n/\\\\t/\\\\\\\\/引号，其余保留反斜杠（与 JS 不同）。\n  不支持：函数字面量、if/for；页面逻辑放 Runtime.evaluate 的 expression。\n",
    );
    out.push_str(
        "\nEnvironment Variables:\n  BROWSE_PORT          daemon 端口（default: 9880）\n  BROWSE_NAME          命名实例：状态目录加派生端口 9900-9999 隔离，多实例并行\n  BROWSE_CHROME        chrome 路径（default: 走发现序：显式、托管 pin、祖先部署、常规路径）\n  BROWSE_PROFILE       spawn 引擎 user-data-dir（default: 固定 engine-profile，down 不删）\n  BROWSE_CDP_WS        钉死连接的 WS URL\n  BROWSE_NO_ATTACH=1   跳过附着探测，强制 spawn 隔离实例\n  BROWSE_EVAL_TIMEOUT  单次求值超时秒数（default: 300）\n",
    );
    out.push_str("\n退出码：\n  0 成功 / 1 执行失败 / 2 用法错\n");
    out
}

/// 渲染紧凑 LLM 清单 `llms.txt`（索引层，一行一命令）。
pub fn render_llms() -> String {
    let mut out = String::from(
        "# browse-rs 命令清单\n\n给 agent 用的 clean-chrome browse CLI。\
         方言片段形态：browse '<片段>'；错误一律带 CTA「下一步」。\n\n",
    );
    for kind in [CmdKind::Cli, CmdKind::Global, CmdKind::Session] {
        out.push_str(&format!(
            "## {}\n\n| 命令 | 说明 |\n| --- | --- |\n",
            kind.label()
        ));
        for c in COMMANDS.iter().filter(|c| c.kind == kind) {
            out.push_str(&format!("| `{}` | {} |\n", c.signature, c.description));
        }
        out.push('\n');
    }
    out.push_str(
        "完整版见 llms-full.txt；机器可读契约见 browse.schema.json；\
         Rust API 文档见 docs/aidoc/llms.txt；二进制直出 browse --llms\
         （--full / --json 同源）。\n",
    );
    out
}

/// 渲染完整 LLM 清单 `llms-full.txt`：索引加逐命令参数与示例。
pub fn render_llms_full() -> String {
    let mut out = render_llms();
    out.push_str("\n---\n");
    for c in COMMANDS {
        out.push_str(&format!("\n## `{}`\n\n{}\n\n", c.signature, c.description));
        if !c.args.is_empty() {
            out.push_str("参数：\n");
            for a in c.args {
                let d = a.default.map(|d| format!("，缺省 {d}")).unwrap_or_default();
                out.push_str(&format!(
                    "- `{}`（{}{}）{}\n",
                    a.name,
                    a.ty,
                    d,
                    if a.required { "，必填" } else { "" }
                ));
            }
        }
        out.push_str(&format!("\n```js\n{}\n```\n", c.example));
    }
    out
}

/// 把派生文件写进目录（维护命令 `browse --gen-surface <dir>` 用）：
/// `browse.schema.json` / `llms.txt` / `llms-full.txt`。
///
/// # Errors
///
/// 写盘失败（目录不可写）。
pub fn write_surface_files(dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::write(
        dir.join("browse.schema.json"),
        serde_json::to_vec_pretty(&render_schema()).unwrap_or_default(),
    )?;
    std::fs::write(dir.join("llms.txt"), render_llms())?;
    std::fs::write(dir.join("llms-full.txt"), render_llms_full())?;
    Ok(())
}
