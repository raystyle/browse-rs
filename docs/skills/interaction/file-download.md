# 文件上传与下载

场景：带附件表单、导出报表类站点。

- 上传：`dropFiles(ref, [绝对路径…])`（DOM.setFileInputFiles 直灌，multiple 支持，路径预检不存在即报错）；路径是 daemon 侧的文件系统
- 下载触发：导航型下载的回执是 `net::ERR_ABORTED`（导航让位下载，正常信号）：裸调 `Page.navigate`，不要用 goto（会当失败报）
- 下载行情：`downloads({since})`（guid/url/filename/state/bytes）；落盘：`downloadPath(guid, timeoutS)` 等 completed 给路径
- 测试通道：`routeMock(pattern, body, {headers: {"Content-Disposition": "attachment; filename=x.txt"}})` 触发真下载（#38）
- begin 事件与导航回执有竞速：垫一拍轮询再查 downloads()
