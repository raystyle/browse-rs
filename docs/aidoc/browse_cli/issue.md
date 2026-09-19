# browse-cli::issue

issue 通道客户端（REQ-057 契约，issues.ohmygh.com）：缺陷一键反馈。

提交自动署名 `tool=browse` 加版本（编译期 Cargo 版）加平台加主机名，
客户端先做与 Worker 同形的校验与截断（title 1 至 200、body 至多 20000、
version 40、platform 与 host 64）；读面 list 与 show 走 GET。
`BROWSE_ISSUES_API` 覆写基址（测与灰度，同 omc 的 OMC_ISSUES_API 惯例）。

## Functions

- `dry_run` — 预览一条 issue 载荷（#57 G6 `--dry-run`）：与 [`new`] 同规校验，不发
- `list` — 列 issue：`GET /api/issues?tool=&status=&limit=&before=`（新到旧，limit 1
- `new` — 提交一条 issue：`POST /api/issues`，回执 `{ok, id, url}`（url 即详情页）。
- `show` — 看 issue 详情：`GET /api/issues/<id>`。

## Constants

- `ISSUES_API` — issue 服务缺省基址（Worker 加 D1 真源，REQ-057）。

