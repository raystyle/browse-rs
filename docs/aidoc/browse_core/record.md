# browse-core::record

录制：`Page.startScreencast` 帧流落盘（方言无回调，泵任务代收）。

方言没有事件回调，帧流由常驻泵任务消费：`drain_events` 取帧 ->
解 base64 写 PNG -> 按帧自带的 sessionId 回 `Page.screencastFrameAck`
（不 ack 的话 chrome 只发头几帧就等住）。`recordStop` 停泵、末冲一次、
`Page.stopScreencast`，回 `{frames,bytes,dir}`。

注意：帧也走事件缓冲（上限 1000），长录制会挤掉旧事件；录短段，
要完整事件流先 peek 再录。

## Functions

- `start` — 开始录制。`opts`（都可省）：`everyNthFrame`（抽帧，源端剪辑）、
- `stop` — 停止录制：停泵 -> 末冲（200ms 窗口内迟到的帧也收）->

## Types

- `Recorder` — 一场进行中的录制：目录、计数器、停泵旗标与泵任务句柄。

