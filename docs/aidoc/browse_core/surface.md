# browse-core::surface

命令面目录（incur-rs 原则的方言版适配）：CLI 子命令、方言全局函数、
session 方法**只在这一处登记为数据**，JSON Schema、LLM 清单
（`llms.txt` / `llms-full.txt`）与技能（`SKILL.md`）全部由
[`render_llms`] 系列派生函数从本目录生成（`browse --gen-surface docs/surface`
重生成），`tests/surface_contract.rs` 锁漂移。

与 incur（derive 宏命令图）的差异：我们的接口面是方言片段而非结构化
参数，故目录手写为 `const`，派生物种取同样的三件（schema/llms/skill）。

## Functions

- `catalog_json` — 返回目录的运行时 JSON，即 hostFunctions 探针的返回体。
- `render_llms` — 渲染紧凑 LLM 清单 `llms.txt`（索引层，一行一命令）。
- `render_llms_full` — 渲染完整 LLM 清单 `llms-full.txt`：索引加逐命令参数与示例。
- `render_schema` — 渲染 JSON Schema 包：每命令一个 definition，输入按目录、输出是运行时 JSON。
- `render_skill` — 渲染技能文件 `SKILL.md`（incur 同款 frontmatter 契约）。
- `write_surface_files` — 把三件派生物写进目录（维护命令 `browse --gen-surface <dir>` 用）：

## Types

- `ArgSpec` — 描述一个参数的形状：名、类型、必填与否与缺省。
- `CmdKind` — 命令在目录中的种类：CLI 子命令、方言全局函数或 session 方法。
- `CmdSpec` — 命令目录里一条命令的登记项。

## Constants

- `COMMANDS` — 全量命令目录，CLI/方言/session 三面的单一真相源。

