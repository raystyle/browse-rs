# browse-cli::fetch

一次性只读抓取（#50）：HTTP 优先，三条件升级引擎，markdown 直出。

- HTTP 腿：reqwest 直取（零浏览器成本），判升级三条件：正文空、命中
  墙词、正文少于 20 词。
- 引擎腿：经 daemon POST /eval 走 goto 加页内抽取（启发式标题加段落，
  非 Readability；v1 口径已在 surface 披露）。

## Functions

- `fetch` — 抓取入口：HTTP 优先，命中升级条件转引擎腿。
- `needs_upgrade` — 升级判定的三条件（#50）：正文空、墙词、少于 20 词。

