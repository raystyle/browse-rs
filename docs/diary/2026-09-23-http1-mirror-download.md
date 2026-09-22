# 2026-09-23 下载腿 HTTP/1.1 钉死批（总台令）

背景：总台定位 browse.ohmygh.com 下载慢根因 = openwrt 透明代理链对 HTTP/2 的字节悬崖（1,720,320B 精确断流，三路径对照；强制 http/1.1 同路径 1.7MB/s 全量通过），判定 `browse update` 下载腿（reqwest，h2 默认）正踩此坑。令：下载腿对镜像域强制 HTTP/1.1 加对照测例；顺带播种腿 rclone stable 段补 `Cache-Control: public, max-age=3600, stale-while-revalidate=86400`。

## 落地

- `self_update.rs`：抽出 `update_http_client(timeout)`（`.http1_only()` 加 connect 30s 加可变 timeout），判新腿（60s）与下载腿（300s）共用；对照测例 `mirror_download_leg_forces_http1_above_cliff`（mock 镜像记录请求首行，取 2.5MB 资产断言镜像域首行恒 `HTTP/1.1` 且全量送达过 sha 锚）。
- `release.yml`：stable 滚动段 rclone 头 `max-age=60` 改 `public, max-age=3600, stale-while-revalidate=86400`；版本段保持 immutable、latest 标记保持 `max-age=60`（判新新鲜度，见下裁定）。

## 关键事实（评审待对齐）

**本仓 reqwest 是 http1-only 编译面**：workspace `default-features = false` 且 features 只开 `json`/`rustls-tls`（未开 `http2`），`cargo tree` 全图无 `http2` feature、`h2` crate 不在 `Cargo.lock`。故：

1. `browse update` 与 `browse chrome install` 的 reqwest 客户端本就只跑 h1，**当前的慢不属于「browse 侧 h2 悬崖」**（悬崖需 h2 客户端；总台实测的 h2 对照很可能是 curl/浏览器，那些默认协商 h2）。
2. 显式 `.http1_only()` 因此是**防漂移口径**而非即时修速，它锁死「日后任一依赖经 feature 统一化打开 `reqwest/http2`」时镜像腿不被漂回 h2 协商；这也是本次改动的主要价值。
3. 若总台观测的慢确在 browse 侧复现，需另查原因（如透明代理对 h1 大响应的其它行为、或镜像 CDN 回源）。本批 stable 段长缓存加 SWR 属 CDN 侧缓解。

## 裁定过程

- 播种头目标段：总台原话「rclone 头补 {值}」未点名段；按 build-release 公共契约家规（版本段 immutable 长缓存、stable 段短缓存），采取 stable 段改值、版本段不动；latest 判新标记维持 `max-age=60`（判新要新鲜，长缓存会迟滞发现新版本），待总台确认是否也要改标记。

[实证: fmt、clippy -D warnings、test --workspace、doctest、e2e、cargo doc、aidoc --check --strict、surface_contract、PEVO；本批 CI 见回执]
