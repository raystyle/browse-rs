# docs 文档地图

> 契约在代码，文档是投影。本图只答一件事：想找某类信息先去哪。生成物勿手改。

## 活跃体系

| 目录 | 回答的问题 | 说明 |
|---|---|---|
| [adr/](adr/) | 为什么选这个 | 不可逆决策记录，frontmatter 状态机 |
| [requirements/](requirements/) | 要做什么、验收什么 | REQ 登记，实现回填 trace |
| [guides/](guides/) | 任务怎么做 | 操作指南，按需生长 |
| [diary/](diary/) | 当天发生了什么 | 一天一篇，过程留痕 |
| [research/](research/) | 证据在哪 | SNNN 研究档案；暂空合法 |

红线：diary 与 research 是保留核心结构，不可裁撤（用户裁定 2026-09-16）；guides 按需生长，不一次建全。

## 投影与专档

| 路径 | 性质 | 重生成 |
|---|---|---|
| [aidoc/](aidoc/) | Rust API 投影（生成物，勿手改） | cargo aidoc |
| [surface/](surface/) | CLI 命令面投影（生成物，勿手改） | cargo run -p browse-cli -- --gen-surface docs/surface |
| [architecture.md](architecture.md) | 结构专档（现在怎么拼，随结构同 PR 更新） | 手写 |

## 门禁

- 骨架合规：PEVO_CHECK_ALLOW="^docs/aidoc/" uv run ~/repos/ProjectEvo/plugins/project-evo/skills/dev-evo/scripts/check.py . 退出码 0。豁免正则在册一处：aidoc 条目分隔符 em dash 是 cargo-aidoc 渲染格式（无开关可改），路径级放行 docs/aidoc/，其漂移真门禁是 cargo aidoc --check --strict
- 投影漂移：cargo aidoc --check --strict 与 tests/surface_contract.rs
