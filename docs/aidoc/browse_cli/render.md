# browse-cli::render

求值结果的打印面：大值自动落盘（artifact/checkpoint 的降级形态，
用户裁定）。daemon 与 vars 表不动，只保护 agent 上下文不被 10MB JSON 淹没。

## Functions

- `render_or_drop_sync` — 渲染求值结果；超阈值时写文件并返回提示行（含路径与预览），

## Constants

- `DROP_THRESHOLD` — 序列化后的打印串超过本阈值（字节）即落盘，保护 agent 上下文。

