# browse-cli::fetch

一次性只读抓取（#50/#56）：HTTP 优先，三条件升级引擎，markdown 直出。

- HTTP 腿：reqwest 直取（零浏览器成本），判升级三条件：正文空、命中
  墙词、正文少于 20 词。
- 引擎腿：经 daemon POST /eval 走 goto 加页内抽取（启发式标题加段落，
  非 Readability；v1 口径已在 surface 披露）。
- 域名层点名（#56）：两腿回执都按 URL 做 goto 同口径匹配，命中
  workspace 站点知识附 domain_skills 与 hint；不依赖引擎（引擎腿
  daemon 侧 goto 的点名回执被抽取串替换，CLI 侧补点）。

## Functions

- `fetch` — 抓取入口：HTTP 优先，命中升级条件转引擎腿。
- `needs_upgrade` — 升级判定的三条件（#50）：正文空、墙词、少于 20 词。

