# browse-core::surface

命令面目录（incur-rs 原则的方言版适配）：CLI 子命令、方言全局函数、
session 方法**只在这一处登记为数据**，JSON Schema、LLM 清单
（`llms.txt` / `llms-full.txt`）与技能（`SKILL.md`）全部由
[`render`] 系列从本目录派生（`browse --gen-surface docs/surface`
重生成），`tests/surface_contract.rs` 锁漂移。

与 incur（derive 宏命令图）的差异：我们的接口面是方言片段而非结构化
参数，故目录手写为 `const`，派生物种取同样的三件（schema/llms/skill）。

## Functions

- `catalog_json` — 目录的运行时 JSON（hostFunctions 探针的返回体）。
- `render_llms` — 紧凑 LLM 清单（llms.txt）。
- `render_llms_full` — 完整 LLM 清单（llms-full.txt）：索引 + 逐命令参数与示例。
- `render_schema` — JSON Schema 包（每命令一个 definition；输入按目录，输出是运行时 JSON）。
- `render_skill` — 技能文件（SKILL.md，incur 同款 frontmatter 契约）。
- `write_surface_files` — 把三件派生物写进目录（维护命令 `browse --gen-surface <dir>` 用）：

## Types

- `ArgSpec` — 一个参数的形状。
- `CmdKind` — 命令种类。
- `CmdSpec` — 一条命令的登记项。

## Constants

- `COMMANDS` — 命令目录（单一真相源）。

