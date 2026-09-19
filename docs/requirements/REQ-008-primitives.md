---
id: REQ-008
title: 批 3 原语补全池（issue #35/#38/#40/#41+#24/#42，滚动切片）
status: draft
priority: should
trace: null
---

# REQ-008：批 3 原语补全池（issue #35/#38/#40/#41+#24/#42，滚动切片）

## Scenario

交互、文件、媒质、视觉、storage 五族原语对照 playwright 系补全；互不依赖，按小批滚动交付，本 REQ 随切片回填。

## Criteria

- [x] #42 storage 颗粒度：cookies/cookieGet/cookieSet/cookieDelete/cookiesClear 加 local/session 四对 CRUD（键值 JSON 序列化内嵌防注入；e2e 覆盖中文往返、sessionClear 不动 local、http 源 setCookie 回读）
- [ ] #35 鼠标原语：mouseMove/mouseDown/mouseWheel、button 与 clickCount、dropFiles
- [x] #40 a11y 媒质仿真族：emulateMedia({colorScheme, reducedMotion, forcedColors, prefersContrast, media}) 加清除（e2e：dark 加 print matchMedia 双证加 clear 还原）
- [ ] #38 文件上传与下载捕获：setFileInput 与 downloads 落盘
- [ ] #41 加 #24 残余：highlight 持久高亮、元素级截图、hires、annotate
