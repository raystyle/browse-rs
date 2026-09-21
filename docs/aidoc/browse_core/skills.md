# browse-core::skills

技能触发层（#50/#51）：goto 导航回执的条件附加面。知识全文存
workspace 单仓（[`crate::workspace`]，github.com/raystyle/browse_workspace），
本模块只做「点名」：命中附加清单与 hint（读全文命令），未命中或
关闭时一键不加，回执与现状逐字节一致。

## Functions

- `domain_hint` — 域名层 hint 字段值（#50）：读全文命令字串。
- `domain_segment` — URL 到域名段（#50，纯函数）：先做 http(s) 门禁（[`cdp::session::url_host`]
- `domain_skills_enabled` — #50 域名层是否开启：`BROWSE_DOMAIN_SKILLS` 未设或非 0/false 即开。
- `list_domain_skills` — 列 `<ws>/domain-skills/<段>/` 的技能文件名（#50，纯函数）：只收

