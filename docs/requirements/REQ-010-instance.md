---
id: REQ-010
title: 实例编排批（issue #48/#28）
status: draft
priority: should
trace: null
---

# REQ-010：实例编排批（issue #48/#28）

## Scenario

无头引擎要从附着浏览器继承指定域登录态（全量导出过宽）；权限弹窗与 UA/CH 高熵字段需要引擎级处理。

## Criteria

- [x] #48 up --headless --cookies 域csv：从附着浏览器只读热迁（后缀域匹配、源零写回、无源报错指 storageState 往返）加 cloneCookies(domains) 宿主函数
- [ ] #28 开关级权限自动授予与 UA/CH 原生覆写（引擎侧 clean-chrome 协同）
