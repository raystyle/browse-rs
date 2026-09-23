# browse-core::skills

技能触发层（#50/#51/#56）：goto 导航回执与 fetch/detect 回执的条件
附加面。知识全文存 workspace 单仓（[`crate::workspace`]，
github.com/raystyle/browse_workspace），本模块只做「点名」：命中附加
清单与 hint（读全文命令），未命中或关闭时一键不加，回执与现状逐字节
一致。

## Functions

- `domain_hint` — 域名层 hint 字段值（#50）：读全文命令字串。
- `domain_segment` — URL 到域名段（#50，纯函数）：先做 http(s) 门禁（[`cdp::session::url_host`]
- `domain_skills_enabled` — #50 域名层是否开启：`BROWSE_DOMAIN_SKILLS` 未设或非 0/false 即开。
- `list_domain_skills` — 列 `<ws>/domain-skills/<段>/` 的技能文件名（#50，纯函数）：只收
- `page_fields` — 把探测结果变成回执附加键对（#51，纯函数）：framework 形合法出
- `page_skills_enabled` — #51 页面层是否开启：`BROWSE_PAGE_SKILLS` 未设或非 0/false 即开。
- `parse_probe` — 解析探测回执（#51，纯函数）：吃 Runtime.evaluate `/result/value` 的
- `url_domain_fields` — URL 到域名层键对（#56，fetch 腿的口径源）：与 goto 同口径的域名段
- `url_domain_fields_multi` — #63 多根域名层键对：序首命中段（有文件）的根整胜，未命中零键。
- `verdict_fields` — detect() 回执的技能附加入口（#56）：判读经 [`verdict_page_slugs`]
- `verdict_page_slugs` — detect() 判读到 page-skill slug 的映射表（#56，纯函数）：challenged

## Constants

- `FROZEN_PAGE_SLUGS` — #51 冻结 slug 名单（探测与回执的合法值域；页面可控串注入面的
- `PAGE_PROBE_JS` — #51 页面特征探测 JS：自包含 IIFE，恒返合法 JSON 字符串（连异常

