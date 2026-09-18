# browse-core::self_update

browse 自更新（用户令 2026-09-18；对齐 build-release 公共契约第六节
双通道）：GitHub Releases latest 判新（semver 只升不降）-> 下载本平台
资产（自家镜像 stable 滚动段优先，GitHub 回落，资产与边车恒同源）->
`.sha256` 边车锚校验（与发布器同 digest 判据，不符即拒不回落）->
解包取二进制 -> 原子自替换（同目录暂存防跨文件系统 rename，pid 后缀
防并发互踩，`--version` 自证带重试，证败回滚并复核终态）。

ark 管理的安装拦自更新走 ark 单通道（判据：exe 同目录 `ark-managed`
落痕，或用户面 bin 目录存在指向本 exe 的符号链接入口即 ark 布局；
落痕生产者契约随家族统一标准派 ark 侧）。

## Functions

- `asset_name` — 某版本的本平台资产名（`v` 前缀形，与 release 六件命名一致）。
- `extract_binary` — 从发布包解出 browse 二进制到暂存目录。
- `latest_browse_version` — 发现最新版本号。
- `swap_binary` — 自替换二进制并自证回滚。
- `update_self` — `browse update` 的自更新全链入口。

## Constants

- `GITHUB_REPO` — GitHub 仓（latest API 判新与回落下载）。
- `RELEASE_MIRROR` — 镜像基址常量。

