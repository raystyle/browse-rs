# browse-core::workspace

workspace 单仓管理（#50/#51 配套）：站点与机制知识仓
`github.com/raystyle/browse_workspace` 的安装、更新与仓内读取。

何时用：`browse workspace` 命令族的全部后端（status/install/update/
list/site/page）。git 维护走 shell-out `git`（不引 git crate）：install
是 clone、update 是 `pull --ff-only`，本地修改可手工 commit/push 回推
同一远端。仓根定位见 [`crate::paths::workspace_dir`]。

## Functions

- `install` — `browse workspace install`：git clone 种子仓到 `root`。clone 不带
- `list_json` — `browse workspace list` 的机器面：`{domains: [{segment, files}],
- `read_page` — `browse workspace page <slug>`：读 `page-skills/<slug>.md` 全文。
- `read_site` — `browse workspace site <段>[/<文件>]`：只给段时打该段清单（每行
- `status_json` — `browse workspace status` 的机器面：`{installed, root, gitPresent,
- `update` — `browse workspace update`：脏树先拒（本地修改未提交会挡 fast-forward），

## Constants

- `DOMAIN_FILES_CAP` — 域名层点名清单的封顶（#50 验收：回执文件列表封顶 10；list 同口径）。
- `SEED_REMOTE` — 种子仓远端（install 的缺省源；本地修改 push 回推同一远端）。

