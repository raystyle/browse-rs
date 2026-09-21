//! 技能触发层（#50/#51）：goto 导航回执的条件附加面。知识全文存
//! workspace 单仓（[`crate::workspace`]，github.com/raystyle/browse_workspace），
//! 本模块只做「点名」：命中附加清单与 hint（读全文命令），未命中或
//! 关闭时一键不加，回执与现状逐字节一致。

use serde_json::{Value, json};

/// 触发开关的关值判式（纯函数）：值为 `0` 或 `false` 才关（opt-out，
/// 未设或任何其他值都开）。与 `BROWSE_NO_ATTACH` 的 opt-in
/// （`1`/`true` 才生效）方向相反，镜像前代 bh 的 `BH_DOMAIN_SKILLS=0`。
fn is_off(v: Option<std::ffi::OsString>) -> bool {
    v.map(|s| s.to_string_lossy().trim().to_ascii_lowercase())
        .is_some_and(|s| s == "0" || s == "false")
}

/// #50 域名层是否开启：`BROWSE_DOMAIN_SKILLS` 未设或非 0/false 即开。
/// 逐调用读 env 不缓存（paths.rs 先例；也保 e2e 可注入临时仓）。
pub fn domain_skills_enabled() -> bool {
    !is_off(std::env::var_os("BROWSE_DOMAIN_SKILLS"))
}

/// #51 页面层是否开启：`BROWSE_PAGE_SKILLS` 未设或非 0/false 即开。
pub fn page_skills_enabled() -> bool {
    !is_off(std::env::var_os("BROWSE_PAGE_SKILLS"))
}

/// URL 到域名段（#50，纯函数）：先做 http(s) 门禁（[`cdp::session::url_host`]
/// 对 data:/about: 这类形返回的是 scheme 段，不能当 host 用），再取 host、
/// 剥 `www.` 前缀、取首个 `.` 前段。非 http(s) 或空 host 返回 `None`。
/// 多词域名不做特判（bbc.co.uk 的段是 bbc，够用口径）。
///
/// # Examples
///
/// ```
/// assert_eq!(browse_core::skills::domain_segment("https://x.com/a"), Some("x".into()));
/// assert_eq!(
///     browse_core::skills::domain_segment("http://www.github.com/"),
///     Some("github".into())
/// );
/// assert_eq!(browse_core::skills::domain_segment("data:text/html,x"), None);
/// ```
pub fn domain_segment(url: &str) -> Option<String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return None;
    }
    let host = cdp::session::url_host(url);
    let host = host.strip_prefix("www.").unwrap_or(&host);
    let seg = host.split('.').next().unwrap_or("");
    if seg.is_empty() || !seg.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        None
    } else {
        Some(seg.to_string())
    }
}

/// 列 `<ws>/domain-skills/<段>/` 的技能文件名（#50，纯函数）：只收
/// .md/.txt，排序保回执确定（read_dir 顺序不保证），封顶
/// [`crate::workspace::DOMAIN_FILES_CAP`]；目录缺失返回空 vec。
///
/// # Examples
///
/// ```
/// // 缺目录返回空（不报错：未装仓是常态）
/// let v = browse_core::skills::list_domain_skills(
///     std::path::Path::new("/nonexistent-browse-ws"), "x");
/// assert!(v.is_empty());
/// ```
pub fn list_domain_skills(ws: &std::path::Path, segment: &str) -> Vec<String> {
    crate::workspace::domain_segment_files(ws, segment)
}

/// 域名层 hint 字段值（#50）：读全文命令字串。
pub fn domain_hint(segment: &str) -> String {
    format!("browse workspace site {segment}")
}

/// #51 页面特征探测 JS：自包含 IIFE，恒返合法 JSON 字符串（连异常
/// 路径都返 `{framework:null,slugs:[]}` 骨架，Rust 侧静默降级零键）。
/// 何时用：仅 goto 后的技能附加入口发一次（Runtime.evaluate
/// returnByValue，8 秒超时兜底）。边界：TreeWalker 前 800 元素封顶、
/// iframe 判定源封顶 50、不用 getComputedStyle（大页防卡顿）；正文单引号
/// 字串，`r##` 是防御性嵌法（评审 G9：现正文无 `"#` 序列，将来引入
/// 双引号字串也不会提前终结 `r#`）。
///
/// slug 与置信档（#51 冻结清单，检出序恒定）：spa（框架容器键或全局，
/// CONFIRMED）、hydration（`__next_f`/`__NUXT_DATA__`/astro-island/
/// `[q:container]`，CONFIRMED）、shadow-dom（前 800 节点 shadowRoot，
/// CONFIRMED）、iframe（清单非空，CONFIRMED）、iframe-cross-origin
/// （源比对，CONFIRMED）、lazy-scroll（页高 >= 8 倍视口加滚动容器，
/// PLAUSIBLE）、bot-shield（cf-chl DOM CONFIRMED / CF cookie
/// PLAUSIBLE）、login-wall（可见 password 框，PLAUSIBLE）、captcha
/// （recaptcha/hcaptcha/turnstile 特征，CONFIRMED）、service-worker
/// （controller 非空，CONFIRMED）。framework 是 info 字段不占 slug。
pub const PAGE_PROBE_JS: &str = r##"(() => {
  const out = { framework: null, slugs: [] };
  const add = (s, c) => { out.slugs.push({ slug: s, confidence: c }); };
  try {
    const w = window, d = w.document;
    if (!d || !d.body) return JSON.stringify(out);

    // ---- 全局与 hydration 标记（廉价先行；点名收进 slug 段保冻结序）----
    const nuxt = w.__NUXT_DATA__ || w.__NUXT__;
    const next = w.__NEXT_DATA__ || (Array.isArray(w.__next_f) ? w.__next_f : null);
    const astro = d.querySelector('astro-island');
    const qwik = d.querySelector('[q\\:container]');

    // ---- framework（info 字段，不占 slug）----
    let fname = null, fver = null;
    if (w.__vue_app__) { fname = 'vue'; fver = w.__vue_app__.version || null; }
    else if (w.Ember && w.Ember.VERSION) { fname = 'ember'; fver = w.Ember.VERSION; }
    else if (astro) { fname = 'astro'; }
    else if (qwik) { fname = 'qwik'; }
    // react（容器键）/angular（body[ng-version]）/svelte（元素自有键）在游走里补

    // ---- cookie（bot-shield 的 PLAUSIBLE 腿）----
    let ck = '';
    try { ck = d.cookie || ''; } catch (e) {}
    const cfCk = /(?:^|;\s*)(?:cf_clearance|__cf_bm)=/.test(ck);

    // ---- 挑战/验证码 DOM（一次合并选择器，CONFIRMED 腿）----
    let chal = false, cfDom = false;
    try {
      chal = !!d.querySelector('.cf-turnstile,.g-recaptcha,.h-captcha,[data-sitekey],'
        + 'script[src*="challenges.cloudflare.com"],script[src*="recaptcha"],'
        + 'script[src*="hcaptcha.com"]');
      cfDom = !!d.querySelector('cf-chl-running,#challenge-running,#challenge-form,'
        + '.cf-turnstile,script[src*="challenges.cloudflare.com"]');
    } catch (e) {}

    // ---- iframe 清单（判定源封顶 50；跨源比对用 hostname 不带端口）----
    let ifn = 0, xo = 0;
    try {
      const ifr = d.querySelectorAll('iframe');
      ifn = ifr.length;
      const cap = Math.min(ifn, 50);
      for (let i = 0; i < cap; i++) {
        const s = ifr[i].src || '';
        const m = s.match(/^https?:\/\/([^\/:]+)/);
        if (m && location.hostname && m[1] !== location.hostname) xo++;
      }
    } catch (e) {}

    // ---- service worker ----
    let sw = false;
    try { sw = !!(w.navigator && w.navigator.serviceWorker &&
                 w.navigator.serviceWorker.controller); } catch (e) {}

    // ---- lazy-scroll 前置（页高 >= 8 倍视口才细看滚动容器）----
    const vh = w.innerHeight || 0;
    const docH = Math.max(d.documentElement ? d.documentElement.scrollHeight : 0, d.body.scrollHeight);
    const tall = vh > 0 && docH >= 8 * vh;

    // ---- 有界节点游走（TreeWalker 前 800 元素封顶）----
    const CAP = 800;
    let n = 0, shadow = false, react = false, svelte = false, angular = null, pw = false, scrollC = false;
    let tw = null;
    try { tw = d.createTreeWalker(d, NodeFilter.SHOW_ELEMENT, null); } catch (e) {}
    if (tw) {
      let el;
      while (n < CAP && (el = tw.nextNode())) {
        n++;
        if (el.shadowRoot) shadow = true;
        const tag = el.tagName;
        if (tag === 'IFRAME' || tag === 'FRAME') continue;
        // react/svelte 检测：查元素自有属性（DOM 元素的 own props 只有
        // expando 键，react 的 __reactFiber$ 与 svelte 的 __svelte* 都是
        // expando，逐元素全键扫描便宜且比容器 id 白名单更稳）
        if (!(react && svelte)) {
          const keys = Object.getOwnPropertyNames(el);
          for (let k = 0; k < keys.length; k++) {
            const key = keys[k];
            if (!react && (key.lastIndexOf('__reactContainer$', 0) === 0 ||
                key.lastIndexOf('__reactFiber$', 0) === 0)) react = true;
            else if (!svelte && key.lastIndexOf('__svelte', 0) === 0) svelte = true;
            if (react && svelte) break;
          }
        }
        if (angular === null && tag === 'BODY' && el.getAttribute) {
          const v = el.getAttribute('ng-version');
          if (v) angular = v;
        }
        if (tag === 'INPUT' && el.type === 'password' && !pw) {
          try { pw = el.getClientRects().length > 0; } catch (e) { pw = true; }
        }
        if (tall && !scrollC && el.scrollHeight > el.clientHeight + 50 &&
            el.clientHeight > 50) scrollC = true;
      }
    }
    if (angular !== null) { fname = 'angular'; fver = angular; }
    if (react) { fname = 'react'; fver = null; }
    else if (svelte && fname === null) { fname = 'svelte'; fver = null; }

    // ---- slugs（固定序，保回执确定；序即文档冻结清单序）----
    if (react || svelte || fname === 'vue' || fname === 'angular' ||
        fname === 'ember' || fname === 'qwik') add('spa', 'CONFIRMED');
    if (next || nuxt || astro || qwik) add('hydration', 'CONFIRMED');
    if (shadow) add('shadow-dom', 'CONFIRMED');
    if (ifn > 0) add('iframe', 'CONFIRMED');
    if (xo > 0) add('iframe-cross-origin', 'CONFIRMED');
    if (tall && scrollC) add('lazy-scroll', 'PLAUSIBLE');
    if (cfDom) add('bot-shield', 'CONFIRMED');
    else if (cfCk) add('bot-shield', 'PLAUSIBLE');
    if (pw) add('login-wall', 'PLAUSIBLE');
    if (chal) add('captcha', 'CONFIRMED');
    if (sw) add('service-worker', 'CONFIRMED');

    if (fname) out.framework = { name: fname, version: fver };
  } catch (e) { /* 整体降级：空骨架零键附加 */ }
  return JSON.stringify(out);
})()"##;

/// #51 冻结 slug 名单（探测与回执的合法值域；页面可控串注入面的
/// 白名单，评审 F3）。种子仓 `page-skills/<slug>.md` 与此同名同序。
pub const FROZEN_PAGE_SLUGS: &[&str] = &[
    "spa",
    "hydration",
    "shadow-dom",
    "iframe",
    "iframe-cross-origin",
    "lazy-scroll",
    "bot-shield",
    "login-wall",
    "captcha",
    "service-worker",
];

/// 解析探测回执（#51，纯函数）：吃 Runtime.evaluate `/result/value` 的
/// 字符串做 JSON.parse；非对象形态（数组、标量、坏 JSON）返回 `None`，
/// 调用方静默降级。边界：探测串由页内 `JSON.stringify` 产出而页面可
/// 覆写该全局（评审 F3 实证），故值域信任不在本函数在
/// [`page_fields`] 的白名单。
///
/// # Examples
///
/// ```
/// assert!(browse_core::skills::parse_probe(r#"{"framework":null,"slugs":[]}"#).is_some());
/// assert!(browse_core::skills::parse_probe("not json").is_none());
/// assert!(browse_core::skills::parse_probe("[1,2]").is_none());
/// ```
pub fn parse_probe(raw: &str) -> Option<Value> {
    let v: Value = serde_json::from_str(raw).ok()?;
    v.as_object()?;
    Some(v)
}

/// 把探测结果变成回执附加键对（#51，纯函数）：framework 形合法出
/// `("framework", {name,version})`；slugs 过滤后非空出 `page_skills` 与
/// `page_skills_hint`（各 slug 的读全文命令用 `；` 连接）；全空返回
/// 空 vec（调用方零插入）。**白名单是硬边界（评审 F3）**：探测串由页内
/// `JSON.stringify` 产出、页面可覆写该全局伪造回执，故这里逐条校验
/// slug 在 [`FROZEN_PAGE_SLUGS`]、confidence 是两档枚举、去重保序
/// （天然封顶名单长），framework 的 name/version 形与长度也校验；
/// 非法条目静默丢弃，全非法即零键。
///
/// # Examples
///
/// ```
/// let probe = browse_core::skills::parse_probe(
///     r#"{"framework":{"name":"vue","version":"3.5"},"slugs":[{"slug":"spa","confidence":"CONFIRMED"}]}"#,
/// ).unwrap();
/// let f = browse_core::skills::page_fields(&probe);
/// assert_eq!(f.len(), 3, "framework 加 page_skills 加 hint: {f:?}");
///
/// // 页面伪造串：名单外 slug 与非法 confidence 全被滤掉（评审 F3）
/// let forged = browse_core::skills::parse_probe(
///     r#"{"framework":{"name":"pwn"},"slugs":[{"slug":"../../etc/passwd","confidence":"CONFIRMED"},{"slug":"spa","confidence":"MAYBE"},{"slug":"captcha","confidence":"CONFIRMED"}]}"#,
/// ).unwrap();
/// let f = browse_core::skills::page_fields(&forged);
/// let skills = f.iter().find(|(k, _)| *k == "page_skills").unwrap();
/// assert_eq!(skills.1, serde_json::json!([{"slug":"captcha","confidence":"CONFIRMED"}]));
/// ```
pub fn page_fields(probe: &Value) -> Vec<(&'static str, Value)> {
    let mut out = Vec::new();
    if let Some(fw) = valid_framework(probe.get("framework")) {
        out.push(("framework", fw));
    }
    let mut kept: Vec<Value> = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for e in probe
        .get("slugs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let (Some(slug), Some(conf)) = (
            e.get("slug").and_then(Value::as_str),
            e.get("confidence").and_then(Value::as_str),
        ) else {
            continue;
        };
        if !FROZEN_PAGE_SLUGS.contains(&slug)
            || !matches!(conf, "CONFIRMED" | "PLAUSIBLE")
            || seen.contains(&slug)
        {
            continue;
        }
        seen.push(slug);
        kept.push(json!({ "slug": slug, "confidence": conf }));
    }
    if !kept.is_empty() {
        let cmds: Vec<String> = seen
            .iter()
            .map(|s| format!("browse workspace page {s}"))
            .collect();
        out.push(("page_skills", json!(kept)));
        out.push(("page_skills_hint", json!(cmds.join("；"))));
    }
    out
}

/// framework 形校验（评审 F3 白名单的 framework 腿）：name 是非空短串
///（字母数字与连字符，<= 24 字节），version 是串（<= 64 字节）或
/// null/缺省；形不对返回 `None`（不附 framework 键）。
fn valid_framework(v: Option<&Value>) -> Option<Value> {
    let fw = v.filter(|f| !f.is_null())?;
    let name = fw.get("name")?.as_str()?;
    if name.is_empty()
        || name.len() > 24
        || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return None;
    }
    let version = match fw.get("version") {
        None | Some(Value::Null) => Value::Null,
        Some(s @ Value::String(_)) => {
            let sv = s.as_str()?;
            if sv.len() > 64 {
                return None;
            }
            s.clone()
        }
        Some(_) => return None,
    };
    Some(json!({ "name": name, "version": version }))
}

/// goto 回执的技能附加入口（crate 内缝，#50/#51）：按开关分流各层；
/// 任何失败（目录列举失败、探测超时或求值错或解析错）静默返回，绝不
/// 让 goto 失败；未命中或关闭时一键不加（回执与现状逐字节一致）。
/// 调用点在 [`crate::semantic::goto`] 的回执构建处，elapsedMs 在此
/// 之前已固化（探测耗时不算进导航时长）。
pub(crate) async fn augment_goto(s: &cdp::Session, receipt: &mut Value) {
    if domain_skills_enabled() {
        let url = receipt.get("url").and_then(Value::as_str).unwrap_or("");
        if let Some(seg) = domain_segment(url) {
            let files = list_domain_skills(&crate::paths::workspace_dir(), &seg);
            if !files.is_empty()
                && let Some(obj) = receipt.as_object_mut()
            {
                obj.insert("domain_skills".into(), json!(files));
                obj.insert("domain_skills_hint".into(), json!(domain_hint(&seg)));
            }
        }
    }
    if page_skills_enabled() {
        // 8 秒超时兜底防怪页挂死 goto；失败静默降级零键
        let probe = tokio::time::timeout(
            std::time::Duration::from_secs(8),
            s.call(
                "Runtime.evaluate",
                json!({ "expression": PAGE_PROBE_JS, "returnByValue": true }),
            ),
        )
        .await
        .ok()
        .and_then(|r| r.ok())
        .and_then(|r| {
            r.pointer("/result/value")
                .and_then(Value::as_str)
                .and_then(parse_probe)
        });
        if let Some(p) = probe
            && let Some(obj) = receipt.as_object_mut()
        {
            for (k, v) in page_fields(&p) {
                obj.insert(k.to_string(), v);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// is_off：0/false 关，未设、1、任意串都开。
    #[test]
    fn switch_off_values() {
        assert!(is_off(Some("0".into())));
        assert!(is_off(Some("false".into())));
        assert!(is_off(Some("FALSE".into())));
        assert!(!is_off(None));
        assert!(!is_off(Some("1".into())));
        assert!(!is_off(Some("off".into())));
    }

    /// domain_segment：命中、去 www.、首段、大小写、非 http(s)。
    #[test]
    fn segment_extraction() {
        assert_eq!(domain_segment("https://x.com/a"), Some("x".into()));
        assert_eq!(
            domain_segment("http://www.github.com/"),
            Some("github".into())
        );
        assert_eq!(
            domain_segment("https://news.ycombinator.com/item?id=1"),
            Some("news".into())
        );
        assert_eq!(domain_segment("https://X.COM"), Some("x".into()));
        assert_eq!(domain_segment("data:text/html,<p>x</p>"), None);
        assert_eq!(domain_segment("about:blank"), None);
        assert_eq!(domain_segment("https://"), None);
    }

    /// list_domain_skills：排序、扩展过滤、封顶 10。
    #[test]
    fn listing_sorted_filtered_capped() {
        let dir = std::env::temp_dir().join(format!("browse-sk-{}", std::process::id()));
        let seg = dir.join("domain-skills").join("x");
        std::fs::create_dir_all(&seg).unwrap();
        for i in 0..11 {
            std::fs::write(seg.join(format!("f{i:02}.md")), "# f").unwrap();
        }
        std::fs::write(seg.join("skip.js"), "// x").unwrap();
        let files = list_domain_skills(&dir, "x");
        assert_eq!(files.len(), 10, "封顶 10：{files:?}");
        assert_eq!(files[0], "f00.md");
        assert!(!files.contains(&"skip.js".to_string()), "非 md/txt 滤除");
        assert!(list_domain_skills(&dir, "nope").is_empty(), "缺段空表");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// parse_probe：合法对象过、坏 JSON 与非对象形拒。
    #[test]
    fn probe_parse_shapes() {
        assert!(parse_probe(r#"{"framework":null,"slugs":[]}"#).is_some());
        assert!(parse_probe("not json").is_none());
        assert!(parse_probe("[1,2]").is_none(), "数组非对象拒");
        assert!(parse_probe(r#""str""#).is_none(), "标量非对象拒");
    }

    /// 冻结名单与探针发射名单零漂移（评审 G-A）：名单每项探针都有
    /// add( 发射位；反向探针发射的每个 slug 都在名单内（否则会被
    /// 白名单静默滤掉，无键无测试红）。bot-shield 双腿（DOM/cookie
    /// 两置信档）是同 slug 双发射位，按集合比对不按次数。
    #[test]
    fn frozen_slugs_match_probe() {
        let mut emitted: Vec<&str> = PAGE_PROBE_JS
            .lines()
            .filter_map(|l| {
                let idx = l.find("add('")?;
                let rest = &l[idx + 5..];
                let end = rest.find('\'')?;
                Some(&rest[..end])
            })
            .collect();
        emitted.sort_unstable();
        emitted.dedup();
        assert_eq!(emitted.len(), FROZEN_PAGE_SLUGS.len(), "发射集与名单数同");
        for slug in FROZEN_PAGE_SLUGS {
            assert!(emitted.contains(slug), "名单项 {slug} 探针未发射");
        }
        for e in &emitted {
            assert!(
                FROZEN_PAGE_SLUGS.contains(e),
                "探针发射 {e} 不在冻结名单（会被白名单静默滤掉）"
            );
        }
    }

    /// page_fields：空骨架零键；framework 单独可出；slugs 出两键带命令。
    #[test]
    fn probe_fields_shapes() {
        let empty = parse_probe(r#"{"framework":null,"slugs":[]}"#).unwrap();
        assert!(page_fields(&empty).is_empty(), "空骨架零键");

        let fw_only =
            parse_probe(r#"{"framework":{"name":"react","version":null},"slugs":[]}"#).unwrap();
        let f = page_fields(&fw_only);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].0, "framework");

        let full = parse_probe(
            r#"{"framework":{"name":"vue","version":"3.5"},"slugs":[{"slug":"spa","confidence":"CONFIRMED"},{"slug":"shadow-dom","confidence":"CONFIRMED"}]}"#,
        )
        .unwrap();
        let f = page_fields(&full);
        assert_eq!(f.len(), 3, "framework 加 page_skills 加 hint: {f:?}");
        let hint = f.iter().find(|(k, _)| *k == "page_skills_hint").unwrap();
        assert!(
            hint.1
                .as_str()
                .unwrap_or("")
                .contains("browse workspace page spa")
                && hint.1.as_str().unwrap_or("").contains("；"),
            "hint 是读全文命令的连接: {hint:?}"
        );
    }
}
