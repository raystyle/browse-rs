# browse-core::paths

实例命名空间（多实例，ADR-0006）：`BROWSE_NAME` 一个名字同时决定
状态目录与 daemon 端口。

与 bh 的 `BH_NAME` 同型：默认实例一切照旧（`~/.browse-rs`、9880）；
命名实例的状态（daemon 日志、drops、screenshots、录制、engine-profile）
全进 `~/.browse-rs/<name>/`，端口由名字稳定派生（9900-9999）：
引擎 profile 独占，命名实例可各自 spawn chrome 互不锁。

## Functions

- `daemon_port` — 返回本实例 daemon 端口：`BROWSE_PORT` 显式优先，其次按 `BROWSE_NAME`
- `derived_port` — 把名字稳定派生成 9900..=9999 区间的端口，同名恒同口（重启不变）。
- `engine_profile_dir` — 返回本实例引擎 profile（`<state>/engine-profile`；命名实例各自独占，
- `instance_name` — 返回实例名（`BROWSE_NAME` 的非空值）；未设即默认实例（`None`）。
- `state_dir` — 返回本实例状态目录（`%USERPROFILE%\.browse-rs[\<name>]`）。
- `state_dir_for` — 把实例名映射到状态目录的纯函数（同输入恒同输出，可单测）。
- `workspace_dir` — 返回 workspace 仓根：`BROWSE_WORKSPACE` 显式优先，缺省

## Constants

- `DERIVED_PORT_MIN` — 命名实例派生端口的下界（区间 9900..=9999，避开默认 9880 与 bh 的 9876）。

