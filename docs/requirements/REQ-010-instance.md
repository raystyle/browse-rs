---
id: REQ-010
title: 实例编排批（issue #48/#28）
status: implemented
priority: should
trace: 实弹 example.com：geolocation query granted、UA 覆写 TestUA/1.0 加 UA-CH platform TestOS；e2e 随 workspace
---

# REQ-010：实例编排批（issue #48/#28）

## Scenario

无头引擎要从附着浏览器继承指定域登录态（全量导出过宽）；权限弹窗与 UA/CH 高熵字段需要引擎级处理。

## Criteria

- [x] #48 up --headless --cookies 域csv：从附着浏览器只读热迁（后缀域匹配、源零写回、无源报错指 storageState 往返）加 cloneCookies(domains) 宿主函数
- [x] #28 browse 侧：grantPermissions(perms, origin?) 浏览器级授予权限（permissions API 即 granted 实弹）加 emulate userAgentMetadata 原生覆写（UA-CH 高熵字段直传 setUserAgentOverride，platform 实弹 TestOS）；clean-chrome 引擎开关（--auto-grant-permissions）为可选加固非依赖，降后续
