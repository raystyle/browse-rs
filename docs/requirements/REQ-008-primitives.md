---
id: REQ-008
title: 批 3 原语补全池（issue #35/#38/#40/#41+#24/#42，滚动切片）
status: implemented
priority: should
trace: 五片全落（#42 storage、#40 媒质、#35 鼠标、#38 下载、#41+#24 视觉件），各片 e2e 块与评审轮随卷 diary 批 19
---

# REQ-008：批 3 原语补全池（issue #35/#38/#40/#41+#24/#42，滚动切片）

## Scenario

交互、文件、媒质、视觉、storage 五族原语对照 playwright 系补全；互不依赖，按小批滚动交付，本 REQ 随切片回填。

## Criteria

- [x] #42 storage 颗粒度：cookies/cookieGet/cookieSet/cookieDelete/cookiesClear 加 local/session 四对 CRUD（键值 JSON 序列化内嵌防注入；e2e 覆盖中文往返、sessionClear 不动 local、http 源 setCookie 回读）
- [x] #35 鼠标原语：mouseMove/mouseDown/mouseUp/mouseWheel（手势合成，首发吞没实测定谳）、clickAt/clickRef 的 button 与 clickCount、dropFiles、hoverAt 既有（e2e：hover 副作用、右键 contextmenu、双击 dblclick、wheel 路径、双文件灌入）
- [x] #40 a11y 媒质仿真族：emulateMedia({colorScheme, reducedMotion, forcedColors, prefersContrast, media}) 加清除（e2e：dark 加 print matchMedia 双证加 clear 还原）
- [x] #38 文件上传与下载捕获：upload 面由 #35 dropFiles 覆盖（DOM.setFileInputFiles）；downloads({since}) 行情加 downloadPath(guid) 等落盘（ensure 面开 Browser.setDownloadBehavior eventsEnabled；routeMock 加 opts.headers 供 Content-Disposition 触发下载）
- [x] #41 加 #24 残余：highlight(ref,{label}) 持久高亮不挡点击、highlightClear 零残留、annotate(refs) 批量徽标、screenshot opts.ref 元素级 clip 加 hires/scale（e2e：两层高亮、元素截图有界、批量画框加清零）
