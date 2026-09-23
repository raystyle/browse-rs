# 2026-09-23 #63 批（workspace 多仓加载）

背景：用户新需求（2026-09-23）：默认种子仓外挂自定义 SKILL 仓，指定路径加载，格式标准同默认仓，配置固化到 browse 应用目录不靠 env。

## 修法（在哪找改了，怎么找没动）

- paths.rs：workspace_roots 多根序（BROWSE_WORKSPACE env 钉死单根最高 > workspaces.json 配置清单序首优先 > 缺省仓垫底）；配置读写（~/.browse-rs/workspaces.json，坏形回空清单退单仓不致命）
- workspace.rs：read_site_multi/read_page_multi（序首命中，全未命中聚合各根路径）、domain_segment_files_multi（序首有文件的根整胜不混拼保回执确定）、list_json_multi（跨根并集首见去重加 roots 面）
- skills.rs/fetch.rs：goto 与 fetch 的域名层点名切 url_domain_fields_multi
- CLI：workspace add/remove 两面（add 校验目录存在、相对路径绝对化、去重追加；remove 按 PathBuf 或字面匹配）；site/page/list 处理器多根化
- 披露：AGENTS workspace 行、README env 与 usage、getting-started 第十节（「只改在哪找不改怎么找」）；surface 两新条目；常用例循环去重保序（手册 120 帽内）

## 实证

单测 multi_root_first_hit_wins（序首整胜、空仓跳过、site/page 多根）与 multi_root_list_union（并集去重、roots 面）。实弹：自定义仓放 mytest 段与 my-recipe 配方，add 后 list 并集在册、site/page 全文直读、goto 与 fetch 回执点名 custom.md 加 hint（默认仓无此段，自定义盖默认实证）。注记：BROWSE_WORKSPACE env 是 daemon 进程态（冷启动生效），暖 daemon 走其启动时环境（在册「改配置重启 daemon」口径），点名验证时暖态命中属预期非漂移。[实证: fmt、clippy -D warnings、test --workspace 13 套、e2e 真 chrome 5 passed、doc、aidoc --check --strict、surface_contract 8、PEVO 全绿]

## 封版 v0.21.0

能力新增取 minor（REQ-004 判据行）；评审轮与发版随后补记。

## 评审一轮（browse-codex-review）

F1 必修（doc 后补未 regen aidoc，本日第四次同坑）：write_workspace_config 补 # Errors 后投影未跟，已 regen 随批；「最后一次编辑之后重跑 aidoc」与 PEVO 同级内化。G1 采纳：status 的 example 复原（机器面 schema 不带错例），手册帽改走两道：方言节两行并一行加常用例剔除 --serve 维护项（精选语义，目录与 --full 面全量不动）；G2 采纳：措辞改「同段被自定义仓覆盖时以自定义仓为准，未覆盖段默认仓仍可用」（CHANGELOG 与 surface 同步）；G3 采纳：add/remove canonicalize 规范化（符号链接别名与 ./ 形归一，双形匹配删除）；G4 采纳：坏形一次性告警（OnceLock 每进程一次，热路径不刷屏），行为仍退单仓。
