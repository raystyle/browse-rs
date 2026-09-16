# browse-core::chrome_mgr

内嵌 Chromium 版本管理器（ADR-0007）：各版本 clean-chrome 在本仓应用
数据目录下版本化管理（安装、pin、升级、体检）。

布局：`<state>/chromium/<version>/`（每版本一目录，旧版保留可回退），
登记 `<state>/chromium/manifest.json`（已装版本 + pin 指向）。
引擎 user-data 不在版本目录（engine-profile 跨版本持久，升级零迁移）。

安装源两形（ADR-0007）：本地目录导入（SxS 部署形态，`chromium-<ver>/`
整目录复制）；R2 镜像版本段下载（chrome.ohmygh.com，`<ver>/<asset>` 加
同名 `.sha256` 边车锚，总台热验 2026-09-17 回执；资产名是暂定约定，
候 clean-chrome 首版资产定标，`BROWSE_CHROME_ASSET` 可覆写）。

## Functions

- `asset_name` — 资产名暂定约定：`chromium-<version>.zip`（候 clean-chrome 首版资产定标；
- `check_deployed` — 校验某目录是可用的 Chromium 部署（chrome 二进制在位）。
- `chromium_root` — 托管根目录（本实例状态目录下的 `chromium/`）。
- `doctor_json` — 体检：逐版本核对部署在位与登记基线（文件数），回结构化结果。
- `install_from_dir` — 从本地部署目录导入安装一个版本（整目录复制到 `<root>/<version>/`），
- `install_from_mirror` — 从 R2 镜像下载安装一个版本（环境覆写形态；见 [`install_from_mirror_with`]）。
- `install_from_mirror_with` — 带显式镜像基址与资产名的下载安装（env 包装的内核，测试与程序化面）：
- `list_json` — 列已装版本与 pin（给 chromeList 面与 CLI）。
- `manifest_path` — manifest 落盘路径。
- `pinned_chrome` — 托管位解析：pin 版本的 chrome 二进制路径（发现序的托管档）。
- `read_manifest` — 读登记册；根目录不存在或 manifest 缺失视为空册。
- `use_version` — pin 切换到已装版本（引擎发现序的托管位生效点）。
- `version_dir` — 某版本的落位目录（不校验存在）。
- `version_from_dir_name` — 从部署目录名提取版本（`chromium-152.0.7977.84` 出 `152.0.7977.84`；
- `write_manifest` — 写登记册（先建根目录）。

## Types

- `ChromeInstall` — 一条已安装版本的登记项。
- `ChromeManifest` — 版本登记册：已装版本 + 当前 pin。

## Constants

- `DEFAULT_MIRROR` — R2 镜像默认基址（chrome.ohmygh.com 版本段路由，ADR-0007 决策二；

