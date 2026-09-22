# 2026-09-22 #57 批（无显示会话 headless 自动回退）

背景：v0.15.0 五端拉平实弹中发现（issue #57，先 --dry-run 后实发）：lan-ubuntu 经 ssh 跑 browse fetch 引擎腿必挂，engine.log 见 Missing X server or $DISPLAY（ozone 初始化失败即退，DevToolsActivePort 永不落盘）；wsl 端同命令正常因 WSLg 恒供 DISPLAY。当日绕行道（daemon 冷启动加 BROWSE_ENGINE_ARGS=--headless=new）已实证，本批把回退收进 spawn 层。

## 修法（cdp 层统一收口）

- cdp/spawn.rs：headless_fallback 纯函数（已无头/显式旗标/有显示三种情形 None，唯「有头意图加无显示加无显式旗标」补 --headless）加 display_present 探测（unix 看 DISPLAY/WAYLAND_DISPLAY 任一非空，Windows 会话制恒真不触发，ADR-0003 口径不动）；三个 spawn 位（端口态与管道态 unix/win 两实现）同缝接线，显式旗标优先不叠补
- 披露面：README env 表补 BROWSE_ENGINE_ARGS 行带 #57 口径；getting-started 首节补回退语义与 daemon 常驻口径；surface 的 up 述与 llms env 块同步（顺修 DOMAIN/PAGE 两行的 goto 单事件陈述）

## 守卫与实弹

单测四象限锁判定矩阵；e2e 新增 no_display_headless_fallback_spawns（SEQ 窗清 DISPLAY/WAYLAND_DISPLAY，有头意图 spawn 应自动回退并连上，修复前该场景必挂）。[实证: fmt、clippy -D warnings、test --workspace、doctest、e2e 真 chrome 4 passed 80s、cargo doc、aidoc --check --strict 31 件、surface_contract 8 passed、PEVO PASS 10；lan-ubuntu ssh 实弹（验收 4）：0.15.1 临时二进制无 DISPLAY 零绕行 fetch 引擎腿回执可得含 domain_skills 点名，验毕 down 加清理]

## 封版 v0.15.1

修复批取 patch（REQ-004 判据行）。

## 评审与发版（随后补记）

评审轮、推送、v0.15.1 发版与五端拉平、#57 关单走 omc，随后补记。

## 评审一轮（browse-codex-review）

四个点名边界过（接线一致性附 F1 联动注记、空串语义、e2e 还原确定性、版本判据 patch 对齐 #52/#53 先例）。F1 必修（核实为真）：display_present 只排 Windows，macOS 是 Cocoa 非 X11/Wayland、环境恒无 DISPLAY 但有桌面，会把 lan-mac 有头默认静默改无头（证伪验收 2，CI mac 岗单测盖不住）。已修：排除面扩 target_os = macos，文档串改 linux 口径，补 mac 岗 cfg 测试 display_present_true_on_macos 回归锁。G1 采纳：e2e 补 EngineSource::Spawned 断言钉住真走了 spawn 腿（防附着态假绿）。G2 记档不做：display_present 的 linux 腿已被 e2e 与 lan-ubuntu 实弹锁、macOS 腿由 mac 岗 cfg 测试锁，注入化收益边际。
