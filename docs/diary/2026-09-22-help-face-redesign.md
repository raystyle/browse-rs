# 2026-09-22 #55 批（帮助面专业化重排）

背景：用户裁定帮助面不专业：命令墙无分类、描述整段长文、全文携带仓内台账编号（#NN/REQ-063/ADR-0002/评审码/裁定日期），溯源信息属 CHANGELOG 与 diary，不该进二进制对外面。cli-docs 标准节序在位但只到骨架（节序与对齐），深度与分类未做。

## 修法（短述派生制，不立第二真相）

- Commands 分组：HELP_GROUPS 显式归组表（求值与抓取、引擎管理、片段与知识仓、账本与产物、自更新五组），组内名按最长名对齐；help_groups_cover_catalog 守卫锁恰一归组（漏归组与重复都红）
- 短述机械派生：short_desc 取 description 首子句（首个全角括号、冒号、分号或句号前截断），帮助面与 llms/schema 共用同一描述真源、按面分层深度；旗标行另回填默认值（从全文提取 default 补尾，不手维护）
- 编号清洗：surface.rs 全量 description 与 arg 缺省字段去台账引用（147 行描述两遍脚本清洗，语义内容保留只去编号；llms 与 schema 投影随迁同净）
- 片段方言节紧凑化（七行：三例加两行语义边界加一行指 --llms）；环境变量块列对齐修正（BROWSE_ENGINE_ARGS 与 BROWSE_WORKSPACE 原缺列）

## 守卫与实弹

surface_contract 三面更新：help_covers_catalog 计数口径改条目行（组标题不计）加 needle 走 help_display_name（与渲染同源派生）；新增 help_groups_cover_catalog 与 help_face_free_of_internal_ids（（#、REQ-0、ADR-0、评审、用户令、修正令六式不回潮）。[实证: fmt、clippy -D warnings、test --workspace 13 套、doc 干净、aidoc 31 件 strict、surface 投影重生成漂移锁绿、交叉面 --all-targets 零警告、PEVO PASS 10；--help 实弹预览分组排版与短述达意]

事故一记：渲染替换首跑时 python join 误用双反斜杠把 surface.rs 全文件换行吞成字面 \n，git checkout 恢复后改走「Write 落临时文件再拼接」道重放（清洗脚本确定性可重放，零内容损失）；拼接曾吃掉 render_llms 文档注释一行，编译红灯即捕即补。

## 封版 v0.13.1

帮助面文档批取 patch（REQ-004 判据行）；#55 先 --dry-run 预览再实发（issue 55 回执 ok）。评审、发版与五端拉平随后补。

## 评审与发版（待补账）

评审闸门、推送与 CI、v0.13.1 发版、五端 update、#55 关单，随后补。
