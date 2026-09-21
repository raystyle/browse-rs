# 2026-09-21 #54 批（判新镜像面：stable/latest 标记）

背景：browse update 判新腿恒走 api.github.com/releases/latest（镜像纯文件段无版本查询面，self_update.rs 在档），下载腿镜像优先但判新腿不是；用户 pwsh 匿名实弹 403 限流（匿名 60/h 机队共用易撞），整个 update 在判新腿退场，GH_TOKEN 只是把限流面往后挪。用户裁定：镜像 stable 段应放 latest 配置，判新默认自家源（文件名小写 latest，用户令）。

## 修法（判新腿补镜像面）

- release.yml 播种步新增 Seed stable latest marker：printf $VER 经 rclone rcat 写 `stable/latest`（单行纯版本号），回读红灯锚定；sync 带 `--delete-excluded` 会洗非资产文件，标记步恒挂 sync 之后每次发版重写
- latest_browse_version 镜像优先：http_get `stable/latest`（不走 GH 头与限流映射），parse_latest_line 校验形（三段 ASCII 数字、至多 16 字节、无空段、拒 v 前缀），任何不成形（404、超时、垃圾页）静默回落 GitHub API，镜像故障不放大成更新失败；命中与回落各带 stderr 溯源行
- surface 的 browse update 词条随迁（判新读 latest 标记）；BROWSE_RELEASE_MIRROR 覆写对判新腿同样生效（mirror_base 共源），mock 测试族同面受益

## 锁与验收

self_update 测试族加两测：latest_version_mirror_first_and_fallback（mock_mirror 判新腿，镜像命中即零 GitHub 依赖、缺标记回落钉死拒连报 GitHub 源错；HTTPS_PROXY 指向拒连环回隔离同 fetch_asset 先例）与 latest_line_parse_forms（v 前缀、两段四段、空段、错误页全拒）。[实证: fmt、clippy -D warnings、test --workspace 13 套、doc 干净、aidoc 31 件 strict、surface 投影重生成漂移锁绿、交叉面 check --all-targets win-gnu 零警告、PEVO PASS 10]

发版后端到端验收（匿名无 GH_TOKEN 的 browse update 出 upToDate）与 #54 关单随后补。另记：cargo aidoc 写面钉 nightly-2026-07-07 工具链，本端午后被动过不在位，rustup 装回后恢复（check 面不受影响）。

## 封版 v0.13.0

行为变化取 minor（REQ-004 判据行）；#54 先 --dry-run 预览再实发（issue 54 回执 ok）。

## 评审与发版（待补账）

评审闸门、推送与 CI、v0.13.0 发版（seed 写 latest 标记首跑）、端到端匿名 update 验收、#54 关单，随后补。
