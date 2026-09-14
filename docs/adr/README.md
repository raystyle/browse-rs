# ADR 索引

| 编号 | 决定 | 状态 |
|---|---|---|
| [0001](0001-daemon-persistent.md) | 常驻 daemon + CLI 自动拉起 | accepted |
| [0002](0002-dialect-faithful-port.md) | 方言忠实移植，不做子命令操作面 | accepted |
| [0003](0003-attach-first-spawn-fallback.md) | 附着优先缺则自起；spawn 带 --no-sandbox | accepted |
| [0004](0004-auto-connect-attach-first-page.md) | 自动连接 + attach 首个 page target | accepted |
| [0005](0005-pipe-channel.md) | spawn 管道通道（S005，Windows 先行） | accepted |

规则：一条 ADR 一个决定；被替代时 Status 改 `Superseded by ADR-00xx`，原文冻结；Context 写给 18 个月后的陌生人；Consequences 必须有坏的一面。模板见 [0000-template.md](0000-template.md)。
