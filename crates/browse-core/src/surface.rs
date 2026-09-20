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
        name: "js-flag",
        kind: CmdKind::Cli,
        signature: "browse --js '<JS 源码>'",
        args: &[
            arg!("js", "string", true),
            arg!("stdin", "any", false, "空实参时整段管道读"),
        ],
        description: "全量 JS 受限旁路（#22，ADR-0002 修订）：不经方言解析器直发 Runtime.evaluate（returnByValue 加 awaitPromise），return 值序列化回传；模板字符串/正则/函数声明直接写，消多层引号转义。分工：方言管 CDP 编排，--js 管页面逻辑。空实参加管道 = 整段 stdin 一次求值（配 --b64 先解码）。不可序列化值（DOM 节点、Date、Map、RegExp 等，含容器内元素）在 returnByValue 下序列化成 {}，CLI 按空容器口径零输出；要值就在 JS 里自己 JSON.stringify 或取原语（.textContent/.outerHTML）。与 --repl 不同行。",
        example: r#"browse --js 'return (() => { const f = s => s.length; return f("ab"); })()'"#,
    },
    CmdSpec {
        name: "b64-flag",
        kind: CmdKind::Cli,
        signature: "browse --b64 '<base64>'",
        args: &[arg!("base64", "string", true, "或空参走管道整段解码")],
        description: "base64 通道（#22）：片段实参（或 --js 的 JS 实参）按 base64 解码后再派发；空实参加管道 = 整段 stdin base64 解码（--js 管道版即 cat x.js.b64 | browse --js --b64）。PowerShell 引号与编码面一并绕开；bash 生成 base64 -w0 <文件>，PowerShell 用 [Convert]::ToBase64String。无短参（-b 是 issue new --body 的既有契约）。",
        example: r#"browse --js --b64 $(printf '%s' '1+1' | base64)"#,
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
        signature: "browse up [--headless] [--pipe] [--chrome <path>] [--profile <dir>] [--ws <url>] [--port <p>] [--proxy <url>] [--proxy-bypass <list>] [--isolated] [--idle-timeout <ms>] [--cookies <域csv>]",
        args: &[
            arg!("headless", "boolean", false, "false"),
            arg!("pipe", "boolean", false, "false"),
            arg!("chrome", "string", false, "发现序"),
            arg!("profile", "string", false, "固定 engine-profile"),
            arg!("ws", "string", false),
            arg!("port", "number", false),
            arg!("proxy", "string", false, "无"),
            arg!("proxy-bypass", "string", false, "无"),
            arg!("isolated", "boolean", false, "false"),
            arg!("idle-timeout", "number", false, "3600000"),
        ],
        description: "显式起引擎（附着优先，缺则 spawn clean-chrome 隔离实例；--profile 自定义 user-data-dir，默认固定 profile 持久保存站点会话；--proxy/--proxy-bypass 直通 chrome 代理旗标；--isolated 隔离态 profile 引擎退出即删（给了 --profile 时 isolated 优先）；--idle-timeout 闲置回收毫秒只对新拉起的 daemon 生效）；--cookies 域 csv（#48）起引擎后从附着浏览器只读热迁指定域登录态（源零写回，无附着源即报错指 storageState 整包往返）。",
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
        description: "安装 Chromium 版本并 pin：缺省从镜像下载（版本段路由加 .sha256 锚校验原子落位），给部署目录则本地导入；Windows 落位自动补 AppContainer ACE（#32，纯形带沙箱可起），回执 appContainerAce。",
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
        description: "托管 Chromium 部署体检（在位/文件基线/pin；AppContainer ACE 仅 Windows 有值，缺则 hints 给 icacls 修法，#32）。",
        example: "browse chrome doctor",
    },
    CmdSpec {
        name: "fetch",
        kind: CmdKind::Cli,
        signature: "browse fetch <url> [--markdown|-m] [--timeout <s>]",
        args: &[
            arg!("url", "string", true),
            arg!(
                "markdown",
                "boolean",
                false,
                "false（v1 同 text，启发式抽取非 Readability）"
            ),
            arg!("timeout", "number", false, "15"),
        ],
        description: "一次性只读抓取（#50）：HTTP 直取零浏览器成本；三条件升级引擎（正文空、墙词、少于 20 词）走 goto 加页内抽取（懒拉起）；回 {via, title, text, upgradedFrom}。对照 waitForResponse：这是无会话一次性抓取。",
        example: "browse fetch https://example.com",
    },
    CmdSpec {
        name: "snippets-list",
        kind: CmdKind::Cli,
        signature: "browse snippets list [site]",
        args: &[arg!("site", "string", false, "目录/路径子串过滤")],
        description: "列片段库（#44，#45 组合生态）：状态目录 snippets/ 下递归走访（<site>/<task>.js 天然分层），每文件取首行 // 注释头当摘要。先查库再写新片段的纪律入口。",
        example: "browse snippets list",
    },
    CmdSpec {
        name: "snippets-show",
        kind: CmdKind::Cli,
        signature: "browse snippets show <rel>",
        args: &[arg!("rel", "string", true)],
        description: "看片段全文（#44）；配合 --js 管道或 POST /eval 即「缺 helper 自己存」闭环。",
        example: "browse snippets show x.test/search-title.js",
    },
    CmdSpec {
        name: "issue-new",
        kind: CmdKind::Cli,
        signature: "browse issue new <标题> [--body <正文>] [--dry-run]",
        args: &[
            arg!("title", "string", true),
            arg!("body", "string", false, "旗标缺省吃管道 stdin"),
        ],
        description: "一键缺陷反馈：自动署名 tool=browse 加版本加平台加主机；--dry-run 同规校验并预览载荷零网络副作用（#57 G6，契约实弹走它别落生产台账）。",
        example: "browse issue new <标题> --body <复现步骤>",
    },
    CmdSpec {
        name: "issue-list",
        kind: CmdKind::Cli,
        signature: "browse issue list [--status <s>] [--limit <n>] [--tool <t>] [--before <id>]",
        args: &[
            arg!("status", "string", false),
            arg!("limit", "number", false, "100"),
            arg!("tool", "string", false, "browse"),
            arg!("before", "id", false),
        ],
        description: "列 issue（新到旧；默认本工具，--tool 换过滤；默认 limit 100 即服务端上限，返回 count 是本次返回条数非在册总数，恰打满时 stderr 出截断提示（#52）；--before <id> 是 keyset 游标翻更早一页（#53））。",
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
        description: "daemon 即只读看板（#54）：GET /（HTML 单页零外部资源，实例概览加活动 tab）与 GET /dashboard/sse（2 秒帧事件流，断线原生重连）；看板无任何写路由。前台跑 daemon（--bind 选监听地址）。daemon 即公开 HTTP 契约（#45）：POST /eval 求值（body {code,new_tab,js}，错误形判 ok 字段）、GET /health 探活；方言做线协议门外语言不限，完整契约与 curl 三例见 docs/guides/http-api.md。",
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
        signature: "snapshot(opts?)",
        args: &[
            arg!("ref", "string", false, "单元素子树（部分展开）"),
            arg!("depth", "number", false, "限深层数（可见树根为第 1 层）"),
        ],
        description: "AX 树快照：nodes 带 role/name/value/childIds/短 ref（e1、e2…），引用表的唯一来源；url/title 走 CDP 查询面，页面主世界零写入（无注入痕）。opts（#36 捕获与检索分离）：ref 取该元素子树（snapshot(e34) 部分展开），depth 限深（大页先浅扫再部分展开省 token）；ref 只盖过滤后的可见集；pierce（#47/#60）：同源 iframe 与 shadow DOM 内容走 DOM.getDocument pierce，跨域 iframe（OOPIF）走子 session AX 树合并（发现加显式 attach；节点带 oopif 标与 ownerSession），都进同一 ref 表可直接 clickRef/fillRef（跨 frame 与跨 OOPIF 坐标已提升；嵌套 OOPIF 只提升一层）；pierce 清单与 AX 投影有结构节点重复（html/body 双份，ref 各自唯一同 backendNodeId），pierce 节点的 name 取 aria-label 或 id（非可见文本），pierce 与 ref 子树组合受限（pierce 节点无 childIds）。",
        example: "const s = await snapshot({depth: 2})",
    },
    CmdSpec {
        name: "findRefs",
        kind: CmdKind::Global,
        signature: "findRefs(query, opts?)",
        args: &[
            arg!("query", "string", true),
            arg!("insensitive", "boolean", false, "false"),
            arg!("context", "number", false, "2（祖先链层数）"),
        ],
        description: "服务端检索 AX 树（#36）：daemon 侧按 name/value 子串匹配（insensitive 开大小写不敏感，不引正则依赖），只回命中节点加 context 层祖先链（grep -C 式），节点带新 ref 可直接 clickRef/fillRef——比全量 snapshot 省一个量级 token；有命中才整表替换引用表（同 snapshot 语义），零命中保留旧表并标 kept_refs:true。检索范围是顶层 AX 树：iframe/shadow 内节点不在其中（#47 实弹），要穿透先 snapshot({pierce: true}) 再按 role/name 取 ref。",
        example: "const f = await findRefs(\"Sign in\", {context: 1})",
    },
    CmdSpec {
        name: "console",
        kind: CmdKind::Global,
        signature: "console(opts?)",
        args: &[
            arg!("since", "number", false, "0（seq 游标）"),
            arg!(
                "minLevel",
                "string",
                false,
                "verbose（error<warning<log/info<debug<verbose）"
            ),
        ],
        description: "控制台消息分级检索（#37，只认活动 tab，他 tab 信号不泄漏）：Runtime.consoleAPICalled 缓冲过滤 minLevel 及以上（档序 error 加 assert 同档 < warning < log/info < debug < verbose，未知名落 verbose）；Runtime 域随 tab 入口自动开（开域前的旧消息收不到），缓冲环形 1000 条超量挤老。回 {count, messages:[{seq,level,text}]}。",
        example: "return await console({minLevel: \"error\"})",
    },
    CmdSpec {
        name: "jsErrors",
        kind: CmdKind::Global,
        signature: "jsErrors(since?)",
        args: &[arg!("since", "number", false, "0（seq 游标）")],
        description: "未捕获 JS 异常列表（#37，只认活动 tab）：Runtime.exceptionThrown 过滤（该事件本就是未捕获面），回 {count, errors:[{seq,text,url,line}]}；开域前与被挤出环形缓冲的旧异常取不到。",
        example: "return await jsErrors()",
    },
    CmdSpec {
        name: "requests",
        kind: CmdKind::Global,
        signature: "requests(opts?)",
        args: &[
            arg!("since", "number", false, "0（seq 游标）"),
            arg!("filter", "string", false, "url 子串过滤"),
        ],
        description: "网络响应摘要列表（#37，只认活动 tab）：Network.responseReceived 映射 {index,requestId,url,status,type,bytes}（index 是过滤后列表序，requestDetail 传同参即对齐）；Network 域随 tab 入口自动开，缓冲环形 1000 条。要单条详情走 requestDetail，要响应体走 responseBody(requestId)。",
        example: "return await requests({filter: \"/api/\"})",
    },
    CmdSpec {
        name: "requestDetail",
        kind: CmdKind::Global,
        signature: "requestDetail(indexOrRequestId, {since, filter}?)",
        args: &[
            arg!("indexOrRequestId", "any", true),
            arg!("since", "number", false, "0（与 requests 同窗）"),
        ],
        description: "单条网络响应详情（#37，只认活动 tab）：index 配同 filter 与 requests() 列表严格对齐（评审 F2 修法），或直接给 requestId 取最新；回 url/status/type/mimeType/headers/bytes，body 另走 responseBody(requestId)。",
        example: "return await requestDetail(0)",
    },
    CmdSpec {
        name: "detect",
        kind: CmdKind::Global,
        signature: "detect()",
        args: &[],
        description: "页面态结构化判读（#49）：{verdict, evidence[], suggestion}。判序 challenged（挑战关键词加 403/503 或短正文）> rate-limited（429）> blocked（403）> stalled（加载失败且未完成）> login-wall（complete 且有密码框，保守独立信号）> blank（complete 但正文与节点双低）> loading > ok；网络信号只认活动 tab（后台 tab 状态码不劫持判读）；信号源是页内探针（一次 evaluate）加事件缓冲网络计数。与 #37 互补：那是流的可观测性，本函数是页面态判官。",
        example: "return await detect()",
    },
    CmdSpec {
        name: "cookies",
        kind: CmdKind::Global,
        signature: "cookies(domain?)",
        args: &[arg!("domain", "string", false)],
        description: "列 cookie（#42）：Network.getCookies 原生字段简表。无参＝当前页 URL 作用域（非全 jar，实弹口径）；给 domain 则按该域 http/https 显式过滤；要跨域清单就逐域调。",
        example: "return await cookies(\"example.com\")",
    },
    CmdSpec {
        name: "cookieGet",
        kind: CmdKind::Global,
        signature: "cookieGet(name)",
        args: &[arg!("name", "string", true)],
        description: "查单条 cookie（#42）：按 name 精确匹配（同名跨域取第一条，需要确定域用 cookies(domain) 再筛），null 即不存在。",
        example: "return await cookieGet(\"session\")",
    },
    CmdSpec {
        name: "cookieSet",
        kind: CmdKind::Global,
        signature: "cookieSet(name, value, opts?)",
        args: &[
            arg!("name", "string", true),
            arg!("value", "string", true),
            arg!("domain", "string", false, "缺省当前页域"),
            arg!("path", "string", false),
            arg!("expires", "number", false, "Unix 秒"),
            arg!("sameSite", "string", false),
        ],
        description: "写单条 cookie（#42）：Network.setCookie，缺 domain 走当前页 URL；回 success 布尔。",
        example: "await cookieSet(\"k\", \"v\", {domain: \"example.com\"})",
    },
    CmdSpec {
        name: "cookieDelete",
        kind: CmdKind::Global,
        signature: "cookieDelete(name, domain?)",
        args: &[
            arg!("name", "string", true),
            arg!("domain", "string", false, "缺省当前页域"),
        ],
        description: "删 cookie（#42）：Network.deleteCookies（CDP 无单数形）按 name 加域删全部匹配（同域同名多 path 一起清，破坏性动作）；domain 缺省当前页域。",
        example: "await cookieDelete(\"k\", \"example.com\")",
    },
    CmdSpec {
        name: "cookiesClear",
        kind: CmdKind::Global,
        signature: "cookiesClear()",
        args: &[],
        description: "清空浏览器全部 cookie（#42，Network.clearBrowserCookies，作用面整浏览器不只当前域，对照 playwright cookie-clear）。",
        example: "await cookiesClear()",
    },
    CmdSpec {
        name: "localGet",
        kind: CmdKind::Global,
        signature: "localGet(key)",
        args: &[arg!("key", "string", true)],
        description: "读 localStorage 单键（#42）：null 即不存在；键值经 JSON 序列化内嵌防注入。",
        example: "return await localGet(\"theme\")",
    },
    CmdSpec {
        name: "localSet",
        kind: CmdKind::Global,
        signature: "localSet(key, value)",
        args: &[arg!("key", "string", true), arg!("value", "string", true)],
        description: "写 localStorage 单键（#42）：回写后回读值（写入即验）。",
        example: "await localSet(\"theme\", \"dark\")",
    },
    CmdSpec {
        name: "localDelete",
        kind: CmdKind::Global,
        signature: "localDelete(key)",
        args: &[arg!("key", "string", true)],
        description: "删 localStorage 单键（#42）。",
        example: "await localDelete(\"theme\")",
    },
    CmdSpec {
        name: "localClear",
        kind: CmdKind::Global,
        signature: "localClear()",
        args: &[],
        description: "清空当前域 localStorage（#42，不动 sessionStorage）。",
        example: "await localClear()",
    },
    CmdSpec {
        name: "sessionGet",
        kind: CmdKind::Global,
        signature: "sessionGet(key)",
        args: &[arg!("key", "string", true)],
        description: "读 sessionStorage 单键（#42）。",
        example: "return await sessionGet(\"tmp\")",
    },
    CmdSpec {
        name: "sessionSet",
        kind: CmdKind::Global,
        signature: "sessionSet(key, value)",
        args: &[arg!("key", "string", true), arg!("value", "string", true)],
        description: "写 sessionStorage 单键（#42）：回写后回读值。",
        example: "await sessionSet(\"tmp\", \"1\")",
    },
    CmdSpec {
        name: "sessionDelete",
        kind: CmdKind::Global,
        signature: "sessionDelete(key)",
        args: &[arg!("key", "string", true)],
        description: "删 sessionStorage 单键（#42）。",
        example: "await sessionDelete(\"tmp\")",
    },
    CmdSpec {
        name: "sessionClear",
        kind: CmdKind::Global,
        signature: "sessionClear()",
        args: &[],
        description: "清空当前域 sessionStorage（#42，不动 localStorage）。",
        example: "await sessionClear()",
    },
    CmdSpec {
        name: "highlight",
        kind: CmdKind::Global,
        signature: "highlight(ref, {label}?)",
        args: &[
            arg!("ref", "string", true),
            arg!("label", "string", false, "徽标文本（annotate 形态）"),
        ],
        description: "持久高亮覆盖层（#41）：2px 橙框不挡点击（pointer-events none），页面坐标绝对定位滚动跟随；label 给定叠编号徽标；幂等（同元素刷新框位）；清场走 highlightClear()。人看 agent 在操作哪个元素。",
        example: "await highlight(\"e3\", {label: \"e3\"})",
    },
    CmdSpec {
        name: "highlightClear",
        kind: CmdKind::Global,
        signature: "highlightClear()",
        args: &[],
        description: "移除全部高亮框与徽标（#41，按 data-browse-hl 属性）。",
        example: "await highlightClear()",
    },
    CmdSpec {
        name: "annotate",
        kind: CmdKind::Global,
        signature: "annotate(refs)",
        args: &[arg!("refs", "array", true, "ref 字符串数组")],
        description: "批量画框加编号徽标（#24 残余）：对一批 ref 各画框并以 ref 本身为徽标文本（与 snapshot 编号天然对齐）；任一项失败先清场再报错（不留半批框）；配合 screenshot() 取证后 highlightClear() 收场。",
        example: "await annotate([\"e1\", \"e3\"])",
    },
    CmdSpec {
        name: "grantPermissions",
        kind: CmdKind::Global,
        signature: "grantPermissions(perms, origin?)",
        args: &[
            arg!(
                "perms",
                "array",
                true,
                "CDP 枚举数组（geolocation/notifications/microphone/camera/…）"
            ),
            arg!("origin", "string", false, "缺省当前页 origin"),
        ],
        description: "开关级权限自动授予（#28）：Browser.grantPermissions 浏览器级授予权限，弹窗不再出；非法枚举（如 clipboard-read）CDP 当场报错透传；回 {granted, origin}。permissions API 查询即 granted。",
        example: r#"await grantPermissions(["geolocation", "notifications"])"#,
    },
    CmdSpec {
        name: "semanticSnapshot",
        kind: CmdKind::Global,
        signature: "semanticSnapshot(opts?)",
        args: &[
            arg!("format", "string", false, "tree（默认）/markdown"),
            arg!("depth", "number", false),
            arg!("selector", "string", false, "CSS 作用域"),
            arg!("box", "boolean", false, "带坐标"),
        ],
        description: "引擎层语义快照（#27 批 1）：Browse.semanticSnapshot 直出紧凑语义树或 markdown（引擎内 AX 遍历，替代 JS 侧 379 节点路径）；引擎须 clean-chrome 扩展域版（--enable-features=CleanChromeBrowseDomain），否则报错带回退口径（snapshot({pierce:true})）。",
        example: r#"return await semanticSnapshot({format: "markdown"})"#,
    },
    CmdSpec {
        name: "cloneCookies",
        kind: CmdKind::Global,
        signature: "cloneCookies(domains)",
        args: &[arg!("domains", "array", true, "域名字符串数组")],
        description: "登录态按域热迁（#48）：从附着浏览器（probe_default 发现）只读取指定域 cookie 后缀匹配，Network.setCookies 灌入当前引擎；源零写回铁律；回 {cloned,matched,domains}。无附着源（probe_default 只探 9222 与默认 profile 的 DevToolsActivePort，其他端口的附着态取不到）报错指 exportStorageState 整包往返；域匹配不区分大小写、输入前导点剥掉；cloned 小于 matched 时回 failed 与 firstError。",
        example: r#"await cloneCookies(["example.com"])"#,
    },
    CmdSpec {
        name: "downloads",
        kind: CmdKind::Global,
        signature: "downloads(since?)",
        args: &[arg!("since", "number", false, "0（seq 游标）")],
        description: "下载列表（#38）：Browser.downloadWillBegin 行情加同 guid 最新 downloadProgress（state/receivedBytes/totalBytes）；落盘目录是状态目录 downloads/，捕获随 tab 入口自动开（Browser 域事件不做活动 tab 过滤）。",
        example: "return await downloads()",
    },
    CmdSpec {
        name: "downloadPath",
        kind: CmdKind::Global,
        signature: "downloadPath(guid, timeoutS?)",
        args: &[
            arg!("guid", "string", true),
            arg!("timeoutS", "number", false, "20"),
        ],
        description: "等下载完成后回落盘路径（#38）：completed 给 named 路径（改名未落时给 guid 原始名候选），canceled 报错，超时带当前态 CTA；guid 来自 downloads()。",
        example: "return await downloadPath(\"<guid>\")",
    },
    CmdSpec {
        name: "screenshot",
        kind: CmdKind::Global,
        signature: "screenshot(path?, full?, opts?)",
        args: &[
            arg!("path", "string", false, "state 目录时间戳名"),
            arg!("full", "boolean", false, "false"),
            arg!("opts", "object", false, "{format,quality,ifChanged}"),
        ],
        description: "页内截图存文件；opts（#24）：format png/jpeg、quality（jpeg 1-100 缺省 80）、ifChanged（与既有文件逐字节相同即 skipped，省读回不省拍摄；字节可比性限同一引擎二进制）。回 {path,bytes,skipped}。同片段 navigate 后的提交窗竞速由 host 层统一等提交（#34 根因级，页面级调用闸到主框架 frameNavigated），本函数另内建 Not attached 有界重试兜底；opts.ref 元素级截图（clip 到 bbox，#41）加 opts.hires/scale 高倍采样。",
        example: r#"return await screenshot(null, false, {ifChanged: true})"#,
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
        description: "开新 tab 并设为活动路由（先 about:blank 再 goto，防竞速假完成）；带 url 时内部等加载预算 15 秒与 goto 缺省对齐（#57 G3）。",
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
        name: "goto",
        kind: CmdKind::Global,
        signature: "goto(url, opts?)",
        args: &[
            arg!("url", "string", true),
            arg!("opts", "object", false, "{timeout:15}"),
        ],
        description: "一步导航（#19）：navigate 加 waitLoad 一体收尾，可选 waitIdle: true（或秒数）再等网络静默，回 {url,title,elapsedMs}；已加载页立即返回。替代 navigate 加 waitFor(loadEventFired) 组合（后者事件已发再注册即假超时，竞速窗），本函数走 readyState 轮询无此窗。timeout 秒口径（#51；不小于 1000 按毫秒误写换算并告警，封顶 600 秒）。下载型导航回 net::ERR_ABORTED 属正常（导航让位下载）：要下文件用裸 Page.navigate 加 downloads()/downloadPath()。",
        example: "return await goto(\"https://example.com\", {waitIdle: true})",
    },
    CmdSpec {
        name: "goBack",
        kind: CmdKind::Global,
        signature: "goBack(delta?)",
        args: &[arg!("delta", "number", false, "1")],
        description: "历史回退 delta 步（#39，越界钳到最早），等加载收尾，回 {steps,url,title}。",
        example: "return await goBack(2)",
    },
    CmdSpec {
        name: "goForward",
        kind: CmdKind::Global,
        signature: "goForward(delta?)",
        args: &[arg!("delta", "number", false, "1")],
        description: "历史前进 delta 步（#39，越界钳到最新），等加载收尾，回 {steps,url,title}。",
        example: "return await goForward()",
    },
    CmdSpec {
        name: "reload",
        kind: CmdKind::Global,
        signature: "reload(opts?)",
        args: &[arg!("opts", "object", false, "{}")],
        description: "刷新当前页（#39）：opts.ignoreCache 走强制刷新；等加载收尾，回 {ignoredCache,url,title,elapsedMs}。",
        example: "return await reload({ignoreCache: true})",
    },
    CmdSpec {
        name: "mouseMove",
        kind: CmdKind::Global,
        signature: "mouseMove(x, y)",
        args: &[arg!("x", "number", true), arg!("y", "number", true)],
        description: "移动鼠标（#35）：触发 CSS :hover 与 mouseenter/leave 路径；hoverAt 是同义坐标版。",
        example: "await mouseMove(120, 40)",
    },
    CmdSpec {
        name: "mouseDown",
        kind: CmdKind::Global,
        signature: "mouseDown(button?, x?, y?)",
        args: &[
            arg!(
                "button",
                "string",
                false,
                "left（right/middle/back/forward）"
            ),
            arg!("x", "number", false, "缺省最近 mouseMove 落点"),
            arg!("y", "number", false),
        ],
        description: "按下不释放（#35）：拖拽与长按语义的半边，与 mouseUp 配对；坐标缺省沿用最近 mouseMove 落点（按下前自动 move 打点）。",
        example: "await mouseDown()",
    },
    CmdSpec {
        name: "mouseUp",
        kind: CmdKind::Global,
        signature: "mouseUp(button?, x?, y?)",
        args: &[
            arg!("button", "string", false, "left"),
            arg!("x", "number", false, "缺省最近落点"),
            arg!("y", "number", false),
        ],
        description: "释放按键（#35）：与 mouseDown 配对；坐标缺省沿用最近落点。",
        example: "await mouseUp()",
    },
    CmdSpec {
        name: "mouseWheel",
        kind: CmdKind::Global,
        signature: "mouseWheel(dx, dy)",
        args: &[arg!("dx", "number", true), arg!("dy", "number", true)],
        description: "滚轮（#35）：像素量（向下滚 dy 正），走 synthesizeScrollGesture 手势合成（导航后首发吞没问题已绕开；落点用最近 mouseMove 位置缺省中上）；SPA 懒加载监听 wheel 时 JS scrollBy 不可替代。要精确单 wheel 事件裸调 Input.dispatchMouseEvent（注意导航后首个会被吞）。",
        example: "await mouseWheel(0, 600)",
    },
    CmdSpec {
        name: "dropFiles",
        kind: CmdKind::Global,
        signature: "dropFiles(ref, paths)",
        args: &[
            arg!("ref", "string", true),
            arg!("paths", "array", true, "daemon 侧绝对路径"),
        ],
        description: "文件灌入 input[type=file]（#35）：DOM.setFileInputFiles 直灌（可靠面，multiple 支持，路径预检不存在即报错防 CDP 静默假成功）；拖拽事件序列用 mouseDown/Move/Up 手拼。",
        example: "await dropFiles(\"e3\", [\"/tmp/a.png\"])",
    },
    CmdSpec {
        name: "clickAt",
        kind: CmdKind::Global,
        signature: "clickAt(x, y, opts?)",
        args: &[
            arg!("x", "number", true),
            arg!("y", "number", true),
            arg!(
                "button",
                "string",
                false,
                "left（right/middle/back/forward）"
            ),
            arg!("clickCount", "number", false, "1（2 即双击语义）"),
        ],
        description: "视口坐标 trusted 点击（点当前可见物，不做遮挡检查）；opts（#35）button 换键、clickCount 双击。",
        example: "await clickAt(120, 40)",
    },
    CmdSpec {
        name: "fillInput",
        kind: CmdKind::Global,
        signature: "fillInput(selector, text, submit?)",
        args: &[
            arg!("selector", "string", true),
            arg!("text", "string", true),
            arg!(
                "submit",
                "any",
                false,
                "false（true 或 {submit:true} 填完顺带 Enter）"
            ),
        ],
        description: "按 CSS 选择器填输入框（SelectAll 不发 Ctrl+A，回读严格验证；select 走 selectOption；submit 填完顺带 Enter（#39）；提交若触发导航或对话框，后续用 goto()/waitLoad() 收尾或先 dialogStatus()。",
        example: "await fillInput(\"#q\", \"hello\", {submit: true})",
    },
    CmdSpec {
        name: "clickRef",
        kind: CmdKind::Global,
        signature: "clickRef(ref, opts?)",
        args: &[
            arg!("ref", "string", true),
            arg!(
                "opts",
                "object",
                false,
                "{button:left,clickCount:1,waitNav:false,timeout:10}"
            ),
        ],
        description: "按 snapshot 短 ref 点击：滚动可见、量中心、遮挡命中测试（被盖即拒绝并报遮挡物）、trusted 派发；opts（#35）button/clickCount 与 waitNav 可同给；opts.waitNav 链接型点击后走有界提交等待（#19）：grace 窗（2 秒或 timeout 较小者）内探到导航即等加载收尾（waitLoad.settled=nav），无导航迹象即返回（waitLoad.settled=no-nav，同文档锚点与 JS 按钮不再误等全窗），timeout 秒口径；同源 iframe 内节点坐标沿 frameElement 链提升到顶层视口系（#47），OOPIF（跨域 iframe）内节点在子 session 量中心后用父页 iframe rect 提升（#60，嵌套 OOPIF 只提升一层）。",
        example: "await clickRef(\"e3\", {waitNav: true})",
    },
    CmdSpec {
        name: "fillRef",
        kind: CmdKind::Global,
        signature: "fillRef(ref, text, submit?)",
        args: &[
            arg!("ref", "string", true),
            arg!("text", "string", true),
            arg!(
                "submit",
                "any",
                false,
                "false（true 或 {submit:true} 填完顺带 Enter）"
            ),
        ],
        description: "按 ref 填输入框：objectId focus、SelectAll+insertText、同节点回读严格验证（OOPIF 内节点全程走子 session，#60）；submit 填完顺带 Enter（#39）；提交若触发导航或对话框，后续用 goto()/waitLoad() 收尾或先 dialogStatus()。",
        example: "await fillRef(\"e2\", \"hello\", true)",
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
        name: "checkRef",
        kind: CmdKind::Global,
        signature: "checkRef(ref)",
        args: &[arg!("ref", "string", true)],
        description: "勾选 checkbox/radio（#39）：读实态不一致才点击，幂等（checkRef 后必 true）；radio 已选中再 uncheck 无意义不点击；非 checkbox/radio 报错指 clickRef。",
        example: "return await checkRef(\"e5\")",
    },
    CmdSpec {
        name: "uncheckRef",
        kind: CmdKind::Global,
        signature: "uncheckRef(ref)",
        args: &[arg!("ref", "string", true)],
        description: "取消勾选 checkbox（#39）：同 checkRef 反向，幂等。",
        example: "return await uncheckRef(\"e5\")",
    },
    CmdSpec {
        name: "pressKey",
        kind: CmdKind::Global,
        signature: "pressKey(key)",
        args: &[arg!("key", "string", true)],
        description: "按键；单键与组合（#23）：Enter 等命名键、单字符、\"Control+a\" 式修饰组合（组合不发字符；Shift 组合不产生大写输入，大写用 typeRef/fillRef；浏览器级快捷键不保证）。",
        example: "await pressKey(\"Control+a\")",
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
        description: "网络拦截：命中的请求本地应答（默认带 ACAO *）；opts.headers（#38）加额外响应头（如 Content-Disposition 触发下载）；headers 追加非覆盖（同名 Content-Type 会并存，换 CT 用 contentType 参数）；跨源页读自定义头要一并给 Access-Control-Expose-Headers。",
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
        signature: "waitLoad(s?)",
        args: &[arg!("s", "number", false, "10")],
        description: "等 document.readyState 到 complete（已加载立即返回）。timeout 秒口径（#51）：不小于 1000 视为毫秒误写，换算并告警，封顶 600 秒（#57 F1 判据收紧，1000 至 3600 不再静默秒直解）；秒直写三位数内（上界 999 秒）；旧毫秒习惯值等价迁移。",
        example: "await waitLoad(8)",
    },
    CmdSpec {
        name: "waitForResponse",
        kind: CmdKind::Global,
        signature: "waitForResponse(pattern, s?)",
        args: &[
            arg!("pattern", "string", true),
            arg!("s", "number", false, "15"),
        ],
        description: "等 URL 命中 glob（与 routeBlock/routeMock 同写法）的最近一个响应完成（#20，只认活动 tab）：回 {requestId,url,status,headers,body,base64Encoded,json}；命中含历史（Network 域须在触发前已开，本函数幂等开收不到已发出的响应），方言无并发，可用形态是触发后等待；体等 loadingFinished 再取（5 秒窗），失败显式 bodyError；base64 自动解码，可解析时附 json。timeout 秒口径（#51：不小于 1000 按毫秒误写换算并告警，封顶 600 秒；秒直写三位数内（上界 999 秒））。",
        example: r#"return await waitForResponse("https://x.test/api*")"#,
    },
    CmdSpec {
        name: "pageEval",
        kind: CmdKind::Global,
        signature: "pageEval(js)",
        args: &[arg!("js", "string", true)],
        description: "全量 JS 一次求值（#22 受限旁路的方言内形态）：直发 Runtime.evaluate（returnByValue 加 awaitPromise），值序列化回传；模板字符串/正则/函数声明直接写。CLI 侧等价 browse --js。",
        example: r#"return await pageEval("(() => { const xs = [1,2,3]; return xs.map(x => x * 2).join(','); })()")"#,
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
        name: "hoverRef",
        kind: CmdKind::Global,
        signature: "hoverRef(ref)",
        args: &[arg!("ref", "string", true)],
        description: "悬停到元素中心（#23）：触发 CSS :hover 与悬停菜单。",
        example: r#"await hoverRef("e3")"#,
    },
    CmdSpec {
        name: "hoverAt",
        kind: CmdKind::Global,
        signature: "hoverAt(x, y)",
        args: &[arg!("x", "number", true), arg!("y", "number", true)],
        description: "悬停到视口坐标（#23）：mouseMoved。",
        example: "await hoverAt(120, 40)",
    },
    CmdSpec {
        name: "dblclickRef",
        kind: CmdKind::Global,
        signature: "dblclickRef(ref)",
        args: &[arg!("ref", "string", true)],
        description: "双击元素（#23）：press/release 两轮 clickCount 递增。",
        example: r#"await dblclickRef("e5")"#,
    },
    CmdSpec {
        name: "dragRef",
        kind: CmdKind::Global,
        signature: "dragRef(srcRef, dstRef)",
        args: &[
            arg!("srcRef", "string", true),
            arg!("dstRef", "string", true),
        ],
        description: "拖拽：源中心按下分八步移到目标中心松开（#23）；覆盖 pointer/mouse 型拖拽，原生 HTML5 draggable 不在序列内。",
        example: r#"await dragRef("e2", "e4")"#,
    },
    CmdSpec {
        name: "keydown",
        kind: CmdKind::Global,
        signature: "keydown(key)",
        args: &[arg!("key", "string", true)],
        description: "按下不松（#23）：裸 keyDown，配合 keyup 成按住语义；组合写法同 pressKey。",
        example: r#"await keydown("Shift")"#,
    },
    CmdSpec {
        name: "keyup",
        kind: CmdKind::Global,
        signature: "keyup(key)",
        args: &[arg!("key", "string", true)],
        description: "松开按键（#23）：裸 keyUp，与 keydown 成对。",
        example: r#"await keyup("Shift")"#,
    },
    CmdSpec {
        name: "typeRef",
        kind: CmdKind::Global,
        signature: "typeRef(ref, text)",
        args: &[arg!("ref", "string", true), arg!("text", "string", true)],
        description: "真实按键序列输入（#23）：focus 后逐字符 keyDown+keyUp，contenteditable/ProseMirror 类编辑器用；普通输入框用 fillRef（insertText）。",
        example: r#"await typeRef("e7", "hello")"#,
    },
    CmdSpec {
        name: "emulateMedia",
        kind: CmdKind::Global,
        signature: "emulateMedia(opts)",
        args: &[
            arg!("colorScheme", "string", false, "dark/light"),
            arg!("reducedMotion", "string", false, "reduce/no-preference"),
            arg!("forcedColors", "string", false, "active/none"),
            arg!(
                "prefersContrast",
                "string",
                false,
                "more/less/no-preference"
            ),
            arg!("media", "string", false, "screen/print"),
        ],
        description: "a11y 媒质仿真族（#40）：Emulation.setEmulatedMedia features 面，五参任给其一以上（非法枚举当场报错列合法值）；页内 matchMedia 感知。单次调用是全量替换：多特征同一次 opts 里给；仿真挂在 target 上跨导航保留，goto 后要 stock 视图记得 emulateMediaClear()。与 emulate({viewport,mobile,userAgent}) 并列成族。",
        example: "await emulateMedia({colorScheme: \"dark\", media: \"print\"})",
    },
    CmdSpec {
        name: "emulateMediaClear",
        kind: CmdKind::Global,
        signature: "emulateMediaClear()",
        args: &[],
        description: "还原媒质仿真（#40）：五特征与媒质全部回 stock。",
        example: "await emulateMediaClear()",
    },
    CmdSpec {
        name: "emulate",
        kind: CmdKind::Global,
        signature: "emulate(opts)",
        args: &[arg!("opts", "object", true)],
        description: "视口与 UA 仿真（#24）：viewport 加 mobile 加 userAgent 全可省；mobile 档同站更省 token；userAgent 只覆写 UA 字符串，UA-CH 高熵字段未动（引擎级覆写是 #28 范围）；userAgentMetadata（#28）随 userAgent 直传 setUserAgentOverride，UA-CH 高熵字段（brands/platform/model 等，architecture 与 platformVersion 为 CDP 必填）原生覆写非 JS 注入。",
        example: "return await emulate({viewport:{width:390,height:844}, mobile:true})",
    },
    CmdSpec {
        name: "setInitScript",
        kind: CmdKind::Global,
        signature: "setInitScript(code)",
        args: &[arg!("code", "string", true)],
        description: "每新文档前注入脚本（#25.2）：反检测补丁、杀 cookie 横幅；SPA 路由不重跑（只对新文档生效），空串清除。",
        example: r#"await setInitScript("window.__no_banner = 1")"#,
    },
    CmdSpec {
        name: "exportStorageState",
        kind: CmdKind::Global,
        signature: "exportStorageState()",
        args: &[],
        description: "导出会话态（#25.3）：cookies 全量加当前页 origin 的 localStorage；多 origin 逐个切 tab 再导。",
        example: "return await exportStorageState()",
    },
    CmdSpec {
        name: "importStorageState",
        kind: CmdKind::Global,
        signature: "importStorageState(state 或 path)",
        args: &[arg!("state", "any", true)],
        description: "导入会话态（#25.3）：吃 exportStorageState 的返回值或其落盘文件路径，回两边计数。",
        example: "return await importStorageState(st)",
    },
    CmdSpec {
        name: "waitIdle",
        kind: CmdKind::Global,
        signature: "waitIdle(s?)",
        args: &[arg!("s", "number", false, "10")],
        description: "等 network 静默（窗口语义：起点前挂着的请求不计）。timeout 秒口径（#51）：不小于 1000 视为毫秒误写，换算并告警，封顶 600 秒（#57 F1 判据收紧，1000 至 3600 不再静默秒直解）；秒直写三位数内（上界 999 秒）。",
        example: "await waitIdle(5)",
    },
    CmdSpec {
        name: "recordStart",
        kind: CmdKind::Global,
        signature: "recordStart(opts?)",
        args: &[arg!("opts", "object", false, "{everyNthFrame:1}")],
        description: "开始录屏（Screencast 帧流落盘；everyNthFrame 源端抽帧、maxWidth/maxHeight 限宽高）；opts（#43）cursor 画跟随光标元素加 showActions 点击处闪圈（screencast 帧可见，回放可读；stop 自动清）。",
        example: "return await recordStart({everyNthFrame: 2})",
    },
    CmdSpec {
        name: "recordChapter",
        kind: CmdKind::Global,
        signature: "recordChapter(title)",
        args: &[arg!("title", "string", true)],
        description: "录制中插章节标记（#43）：按当前帧计数追加到录制目录 chapters.jsonl（文件格式已冻结：每行 {atFrames,title}；消费方回放工具二期，当前仓内只写不读）。",
        example: "await recordChapter(\"登录流程\")",
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
        signature: "session.waitFor(method, undefined, s?)",
        args: &[
            arg!("method", "string", true),
            arg!("s", "number", false, "15"),
        ],
        description: "从事件缓冲取第一个 method 事件（取出即移除；超时报错，秒口径 #51）。只认活动 tab 与 browser 级事件，钉住 session（录制中）与他 tab 的不被误领；要看全缓冲用 peekEvents（不过滤）。竞速警示（#19）：loadEventFired 这类一次性事件可能在你注册前已发（假超时），等加载用 goto()/waitLoad()，等导航事件用 frameNavigated。",
        example: "await session.waitFor(\"Page.frameNavigated\", undefined, 15)",
    },
    CmdSpec {
        name: "waitJs",
        kind: CmdKind::Session,
        signature: "session.waitJs(expression, s?)",
        args: &[
            arg!("expression", "string", true),
            arg!("s", "number", false, "10"),
        ],
        description: "页内谓词轮询（真 V8 表达式），等到即返回真值本身。timeout 秒口径（#51）：不小于 1000 视为毫秒误写，换算并告警，封顶 600 秒（#57 F1 判据收紧，1000 至 3600 不再静默秒直解）；秒直写三位数内（上界 999 秒）。",
        example: "await session.waitJs(\"document.querySelector('#x') !== null\", 5)",
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

    // 意图路由表（#46）：想做什么用什么，一行一带；细节下钻 interaction 层
    out.push_str(
        "\n意图路由（想做什么用什么；机制细节下钻 docs/skills/interaction/，HTTP 契约 docs/guides/http-api.md）：\n  去页面并等可用 goto(url,opts)；等加载 waitLoad(s)；等导航事件 waitFor(frameNavigated,s)\n  找元素 findRefs(q)；拿结构 snapshot({depth}) 或 {ref} 子树；点击/填 clickRef/fillRef（ref 来自最近快照或 findRefs）\n  取回值/正文 pageEval(js) 或 session.Runtime.evaluate；多 tab listPageTargets/switchTab/currentTab\n  hover 菜单 hoverRef；右键双击 clickRef(ref,{button,clickCount})；滚轮 mouseWheel；拖拽 mouseDown-Move-Up 手拼\n  传文件 dropFiles；下文件 裸 Page.navigate 加 downloads()/downloadPath()；等特定响应 waitForResponse(pattern,s)\n  页面打不开/登不上 detect() 七判；console 错误 console({minLevel});JS 异常 jsErrors();请求对账 requests()/requestDetail()\n  仿真 emulate({viewport,UA}) / emulateMedia({colorScheme});复用片段 browse snippets list/show；门外语言 POST /eval\n",
    );
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
        "--idle-timeout <ms>",
        "引擎闲置回收毫秒（伴 up，只对新拉起的 daemon 生效；default: 3600000，0 关闭）",
    ),
    (
        "--isolated",
        "隔离态 profile：引擎退出即删，不留站点痕迹（伴 up）",
    ),
    (
        "--json",
        "JSON 输出（伴 status；--llms --json 出机器形 Schema）",
    ),
    ("--pipe", "spawn 引擎走 CDP 管道（零 TCP 面）"),
    (
        "--proxy <url>",
        "引擎代理 --proxy-server（伴 up；BROWSE_PROXY 同值）",
    ),
    (
        "--proxy-bypass <list>",
        "代理旁路 --proxy-bypass-list（伴 up）",
    ),
    (
        "--secrets <file>",
        "dotenv 密钥文件（#25.4）：片段经 secrets.<NAME> 取值，stdout 与回显脱敏为 ***（防整值外泄，不防片段；落盘工件与网络响应体不脱敏）；键容 export 前缀与行内 # 注释（shell 可直用同文件）；取值看 daemon 启动时仓、展示看本次 CLI 旗标；改密钥重启 daemon",
    ),
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
        "\n片段方言：\n  await session.connect({port:9222})\n  const tabs = await listPageTargets()\n  await session.use(tabs[0].targetId)\n  await session.Page.navigate({url:\"https://example.com\"})\n  await session.waitFor(\"Page.frameNavigated\", undefined, 15)  // 秒；loadEventFired 有竞速窗，等加载用 goto()/waitLoad()\n  支持：字面量/对象/数组/成员/下标/await/const-let-var/return。\n  模板字符串（反引号）raw 语义：内容逐字保留（只有 \\\\` 与 \\\\${ 是转义，模板内 \\\\${ 降格为 ${），可多行，页面代码原样内嵌 expression。\n  普通字符串只转义 \\\\n/\\\\t/\\\\\\\\/引号，其余保留反斜杠（与 JS 不同）。\n  不支持：函数字面量、if/for；页面逻辑放 Runtime.evaluate 的 expression。\n  片段库（#44）：可复用片段存状态目录 snippets/（<site>/<task>.js 分层，首行 // 用途： 注释头），browse snippets list/show 查读，先查库再写新的；执行走 --js 管道或 POST /eval。\n",
    );
    out.push_str(
        "\nEnvironment Variables:\n  BROWSE_PORT          daemon 端口（default: 9880）\n  BROWSE_NAME          命名实例：状态目录加派生端口 9900-9999 隔离，多实例并行\n  BROWSE_CHROME        chrome 路径（default: 走发现序：显式、托管 pin、祖先部署、常规路径）\n  BROWSE_PROFILE       spawn 引擎 user-data-dir（default: 固定 engine-profile，down 不删）\n  BROWSE_CDP_WS        钉死连接的 WS URL\n  BROWSE_NO_ATTACH=1   跳过附着探测，强制 spawn 隔离实例\n  BROWSE_EVAL_TIMEOUT  单次求值超时秒数（default: 300）\n  BROWSE_IDLE_TIMEOUT  引擎闲置回收毫秒，到期退引擎下次求值自动拉起（default: 3600000，0 关闭）\n  BROWSE_PROXY         引擎代理 --proxy-server（与 --proxy 旗标同值）\n  BROWSE_PROXY_BYPASS  代理旁路 --proxy-bypass-list\n  BROWSE_SECRETS       dotenv 密钥文件（--secrets 同值；输出回显脱敏）\n",
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
