# browse-core::workspace

workspace 单仓管理（#50/#51 配套）：站点与机制知识仓
`github.com/raystyle/browse_workspace` 的安装、更新与仓内读取。

何时用：`browse workspace` 命令族的全部后端（status/install/update/
list/site/page）。git 维护走 shell-out `git`（不引 git crate）：install
是 clone、update 是 `pull --ff-only`，本地修改可手工 commit/push 回推
同一远端。仓根定位见 [`crate::paths::workspace_dir`]。

## Functions

- `domain_segment_files` — 列 `<root>/domain-skills/<段>/` 的技能文件名（排序，封顶
- `domain_segment_files_multi` — #63 多根段文件列（技能点名口径）：序首有文件的根整胜（不跨根混拼，
- `install` — `browse workspace install`：git clone 种子仓到 `root`。clone 不带
- `list_json` — `browse workspace list` 的机器面：`{domains: [{segment, files}],
- `list_json_multi` — #63 多根清单：domain 段与 page slug 跨根并集（首见序，段名重复以
- `read_page` — `browse workspace page <slug>`：读 `page-skills/<slug>.md` 全文。
- `read_page_multi` — #63 多根 page 读：序首命中根优先。
- `read_site` — `browse workspace site <段>[/<文件>]`：只给段时打该段清单（每行
- `read_site_multi` — #63 多根 site 读：序首命中根优先（根内逻辑同 [`read_site`]）；全根
- `status_json` — `browse workspace status` 的机器面：`{installed, root, gitPresent,
- `update` — `browse workspace update`：脏树先拒（本地修改未提交会挡 fast-forward），

## Constants

- `DOMAIN_FILES_CAP` — 域名层点名清单的封顶（#50 验收：回执文件列表封顶 10；list 同口径）。
- `SEED_REMOTE` — 种子仓远端（install 的缺省源；本地修改 push 回推同一远端）。

