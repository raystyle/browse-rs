# 应用插件契约（#55）

> 应用是本机生态件：一个可执行文件（pwsh 7 脚本为家族标准载体），子命令路由，经 browse 的 CLI 或 HTTP 面（POST /eval）驱动默认 daemon。不做 MCP、不做 Cloud。

## 契约

1. **载体与路由**：`apps/<name>`（pwsh 脚本或可执行）；参数即子命令（`apps/<name> <sub> [args]`）。`browse <name>` 同款路由留给家族 ark 统一管理，apps/ 目录是仓内样例与约定的权威。
2. **ctx 安全面**：应用只用 browse 的只读与求值面（fetch/snapshot/status/POST /eval）；写操作与引擎生命周期（up/down/engine）不暴露给应用——应用不拥有引擎。
3. **helper 覆盖**：站点级 helper 走 #44 片段库的命名覆盖约定（`snippets/<site>/<task>.js`，`browse snippets list/show` 查读）；应用自带 helper 就存自己的 snippets 命名空间。
4. **实例隔离**：应用需要专属实例时设 `BROWSE_NAME=<app>-<name>`（派生端口与状态目录隔离）；空闲自退沿用 `BROWSE_IDLE_TIMEOUT`。
5. **崩溃边界**：应用是独立进程，崩溃只死自己；daemon 与其他应用不受影响（不共享进程状态）。

## 样例

`apps/web-fetch`（#50 面的薄封装）：`apps/web-fetch https://example.com` 走 `browse fetch`（HTTP 优先、三条件升级、零浏览器成本优先）。

## 测试用例（验收面）

- 样例应用跑通并复用默认 daemon（不另起引擎）
- helper 覆盖：样例存 snippets 后 `browse snippets show` 可读
- 应用崩溃（exit 非 0）不影响 daemon（`browse status` 仍活）
- 专属实例空闲自退（BROWSE_NAME + IDLE_TIMEOUT）
