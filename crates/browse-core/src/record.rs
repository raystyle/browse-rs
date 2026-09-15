//! 录制：`Page.startScreencast` 帧流落盘（方言无回调，泵任务代收）。
//!
//! 方言没有事件回调，帧流由常驻泵任务消费：`drain_events` 取帧 ->
//! 解 base64 写 PNG -> 按帧自带的 sessionId 回 `Page.screencastFrameAck`
//! （不 ack 的话 chrome 只发头几帧就等住）。`recordStop` 停泵、末冲一次、
//! `Page.stopScreencast`，回 `{frames,bytes,dir}`。
//!
//! 注意：帧也走事件缓冲（上限 1000），长录制会挤掉旧事件——录短段，
//! 要完整事件流先 peek 再录。

use anyhow::Result;
use cdp::Session;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

/// 一场进行中的录制：目录、计数器、停泵旗标与泵任务句柄。
pub struct Recorder {
    dir: PathBuf,
    frames: Arc<AtomicU64>,
    bytes: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    handle: tokio::task::JoinHandle<()>,
}

/// 泵的可变共享面（任务与 stop 两侧都要动）。
struct Counters {
    next: u64,
    dir: PathBuf,
    frames: Arc<AtomicU64>,
    bytes: Arc<AtomicU64>,
}

impl Recorder {
    /// 启动面简报（recordStart 的返回值）。
    pub fn brief(&self) -> Value {
        json!({ "dir": self.dir.display().to_string() })
    }
}

/// 开始录制。`opts`（都可省）：`everyNthFrame`（抽帧，源端剪辑）、
/// `maxWidth`/`maxHeight`（缩放）、`quality`（JPEG 质量；本实现恒 PNG，
/// 留作向后兼容）。帧写 `<state>/record-<ts>/frame-NNNNNN.png`。
///
/// # Errors
///
/// 未连接、`Page.startScreencast` 失败、目录建不出来。
pub async fn start(s: Arc<Session>, opts: &Value) -> Result<Recorder> {
    let mut params = json!({ "format": "png", "everyNthFrame": 1 });
    for k in ["everyNthFrame", "maxWidth", "maxHeight"] {
        if let Some(v) = opts.get(k) {
            params[k] = v.clone();
        }
    }
    s.call("Page.startScreencast", params).await?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let dir = crate::paths::state_dir().join(format!("record-{ts}"));
    tokio::fs::create_dir_all(&dir).await?;
    let frames = Arc::new(AtomicU64::new(0));
    let bytes = Arc::new(AtomicU64::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let pump_session = s.clone();
    let pump_counters = Counters {
        next: 0,
        dir: dir.clone(),
        frames: frames.clone(),
        bytes: bytes.clone(),
    };
    let pump_stop = stop.clone();
    let handle = tokio::spawn(async move {
        let mut c = pump_counters;
        while !pump_stop.load(Ordering::Relaxed) {
            pump_once(&pump_session, &mut c).await;
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    });
    Ok(Recorder {
        dir,
        frames,
        bytes,
        stop,
        handle,
    })
}

/// 停止录制：停泵 -> 末冲（200ms 窗口内迟到的帧也收）->
/// `Page.stopScreencast`，回 `{frames,bytes,dir}`。
///
/// # Errors
///
/// `Page.stopScreencast` 失败（帧已在盘上的不回滚）。
pub async fn stop(s: &Session, rec: Recorder) -> Result<Value> {
    rec.stop.store(true, Ordering::Relaxed);
    let _ = rec.handle.await;
    // 末冲：泵的最后一拍 sleep 里到的帧
    let mut c = Counters {
        next: rec.frames.load(Ordering::Relaxed),
        dir: rec.dir.clone(),
        frames: rec.frames.clone(),
        bytes: rec.bytes.clone(),
    };
    pump_once(s, &mut c).await;
    let _ = s.call("Page.stopScreencast", json!({})).await;
    Ok(json!({
        "frames": rec.frames.load(Ordering::Relaxed),
        "bytes": rec.bytes.load(Ordering::Relaxed),
        "dir": rec.dir.display().to_string(),
    }))
}

/// 收一拍：drain 帧事件 -> 写盘 -> 按帧自带 sessionId ack。
/// 单帧失败只丢那一帧（计数不增），不倒整场录制。
async fn pump_once(s: &Session, c: &mut Counters) {
    for ev in s.drain_events("Page.screencastFrame").await {
        let from = ev
            .get("sessionId")
            .and_then(Value::as_str)
            .map(str::to_string);
        let data = ev
            .pointer("/params/data")
            .and_then(Value::as_str)
            .unwrap_or("");
        if !data.is_empty()
            && let Ok(bytes) = crate::js_host::base64_decode(data)
            && !bytes.is_empty()
        {
            let path = c.dir.join(format!("frame-{:06}.png", c.next + 1));
            if tokio::fs::write(&path, &bytes).await.is_ok() {
                c.next += 1;
                c.frames.store(c.next, Ordering::Relaxed);
                c.bytes.fetch_add(bytes.len() as u64, Ordering::Relaxed);
            }
        }
        if let Some(sid) = from {
            let _ = s
                .call_on("Page.screencastFrameAck", json!({ "sessionId": sid }), &sid)
                .await;
        }
    }
}
