# ADR-0003：附着优先缺则自起；spawn 带 --no-sandbox

- Status: accepted
- Date: 2026-09-14
- Deciders: 用户裁定（引擎策略问询）+ 维护者

## Context

clean-chrome 的存在理由就是自动化：`--auto-allow-devtools-connections`
免确认对话框、无参数启动即开 9222（clean-chrome D02-1/D02-2，已三平台验收）。
bh 对普通 Chrome 的铁律（永不 spawn、只附着）源于对话框与用户 profile 污染；
这些前提在 clean-chrome 上不成立。用户裁定：附着优先，缺则自起。

本机实证（2026-09-14）：SxS 部署的 chrome.exe 带沙箱启动即崩
（`Sandbox cannot access executable ... 0x5`，exit 0x80000003）；
ACL 与 EFS 检查均正常，clean-chrome 在 linux 验收同样用过 --no-sandbox
（GOAL 2026-09-13，R001 坑表 18-20）。

## Decision

`Engine::ensure` 顺序：显式 ws/port -> `probe_default()`（`/json/version`@9222
短超时 -> 默认 profile 的 `DevToolsActivePort`）-> spawn 专属实例
（独立 profile `%USERPROFILE%\.browse-rs\engine-profile`、
`--remote-debugging-port=0`、`--no-first-run --no-default-browser-check`、
`--no-sandbox`、可选 `--headless`）。spawn 前清残留 `DevToolsActivePort`。
`browse down` 只终结 spawn 来源（`Browser.close` 优雅 -> `try_wait` 5 秒 ->
taskkill /T 兜底）；附着来源绝不关。因为引擎实例是自动化专属、隔离
profile、不承载用户浏览面，沙箱增益有限而启动即崩是硬故障，所以
spawn 恒带 `--no-sandbox`。

## Consequences

- 好：一条命令可用（无浏览器也行）；人机共存（在跑就附着，不动用户窗口）。
- 好：attach 探测短超时（300ms），冷启动不被拖慢。
- 坏：引擎实例无沙箱：页面漏洞即同权限进程；靠「专属 profile + 不承载
  用户身份 + 用完即杀」收敛暴露面。
- 坏：probe 的 9222 若被非 clean-chrome 的浏览器占用会误附着（概率低，v0.1 接受）。

## Alternatives

- 永不自起（bh 铁律）：浪费 clean-chrome 的定位，用户已否决。
- 永远自起：忽略用户已开的登录态窗口，同样被否决。
- 修沙箱（部署 ACL/复制方式排查）：clean-chrome 侧课题（记 M023 候选），
  browse-rs 不阻塞等待。
