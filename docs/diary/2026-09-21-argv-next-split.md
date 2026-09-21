# 2026-09-21 #53 批（argv 取值口双面拆分，修 0.12.1 G-F 收口回归）

背景：台账实质清零后例行核账（`browse issue list` 看 open 集），裸调当场假报「issue list 旗标 需要一个值」exit 2。追根：06b93f3（#52 批 G-F 收口）把 CLI `next` 取值口从「Err + anyhow exit 1」改为直出 exit 2，但七处子命令旗标收集循环按「参数尽返 Err 即收尾」旧契约写，Err 分支全成死代码：fetch、artifact publish/attest/list、ledger keygen、issue new、issue list 任意调用都在旗标耗尽时假报用法错（带不带旗标都死）。附带踩碎面：`snippets list` 可选位 `[site]`（`.ok()`）裸调恒 exit 2；`workspace site`/`page` 的 Mode 层 G-F 缺参处理器被解析层先拦成死代码；`snippets show` 从无 Mode 层守卫（评审 F1 勘误：初版本档与 CHANGELOG 误称其有，实际 0.12.1 前裸调走 Err 到空串落「片段 不存在」exit 1 误导面，本批补齐）。0.12.1 里连开单工具 `issue new` 自己都是坏的，开单只能用工作树修好的 debug 二进制实弹。

## 修法（一个 next 服务两个契约是根因，拆双口）

- `Args(std::iter::Skip<std::env::Args>)` 实参游标：必值口 `next(flag)` 参数尽即用法错直出 exit 2（G-F 口径原样保留），收集口 `next_opt()` 参数尽返 None 即收尾（旗标循环与可选位专用）
- 七处循环两形态（`while let Ok` 三处、`loop { match }` 四处）与可选位全部切收集口；chrome install 的 from_dir 裸迭代器位一并归口
- bail_arg 措辞中性化：各子命令旗标循环共用一个出口，非 issue 子命令的坏旗标不再误报「issue 参数不认识」（实弹中发现的原有措辞缺陷，同批顺手收口）

## 验收与锁（argv 面此前零锁是漏网主因）

browse-cli 首建 tests/arg_contract.rs（CARGO_BIN_EXE_browse 真二进制四测，零网络）：旗标循环收尾矩阵（issue new --dry-run 出 dryRun:true、artifact publish 缺 --kind 命中必填校验、隔离态 keygen ok:true、裸 snippets list 空库回执、坏旗标/坏值网络前拦截）、必值口负控（--eval 与 fetch 缺 url 仍 exit 2）、Mode 层缺参处理器复活（workspace site 裸调出 G-F 文案）、bail_arg 中性措辞。测试态隔离走 BROWSE_NAME 绝对路径（G7 口径），不碰本机密钥与片段库；spawn 恒 stdin null（issue new 空 body 会读管道）。实弹矩阵同绿（issue list --limit 5 真 GET、artifact list、keygen 在位拒绝守卫）。[实证: fmt、clippy -D warnings、test --workspace 全绿（arg_contract 四测随卷）、test --doc 12、doc 干净、aidoc 31 件 strict、surface 投影逐字节、PEVO PASS 10]

## 封版 v0.12.2

修复批取 patch（REQ-004 判据行）；#53 先 --dry-run 预览零入账再实发（issue 53 回执 ok）；版本头随迁 aidoc/surface 投影（纯版本行 diff）。

## 评审闸门与关单（待补账）

一轮快核（browse-codex-review 重组窗格，deepseek-v4-flash）：(a) F1 必修：snippets show 裸调漂移回 G-F 前 exit 1（从无 Mode 层守卫，初版文案误称「处理器复活」），补 SnippetsShow 空 rel 守卫（workspace site 同款）加 arg_contract 裸调断言，CHANGELOG 与本档措辞同步勘误；(b) CONFIRM（迁移完整：54 处 baseline 逐一比对，args.next 43 加 next_opt 12，无第二解析面）；(c) CONFIRM 带 G1（win-gnu 交叉岗 check 裸跑不编译集成测试，补 --all-targets）与 G-lite（TempState Drop 收渣）；(d) CONFIRM（bail_arg 新串无锁风险，surface 零命中）。二轮快核与终审、推送、#53 关单后补。
