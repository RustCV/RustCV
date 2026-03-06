// 创建一个专用的串行 GCD 队列供 AVCaptureVideoDataOutput 使用。
// 绝对不能使用主队列（main queue），否则回调会堵塞 UI 线程并导致死锁。
//
// dispatch2 API 说明：
//   - DispatchQueue::new(label, attr) → DispatchRetained<DispatchQueue>
//   - attr = None 表示串行队列（DISPATCH_QUEUE_SERIAL）

use dispatch2::{DispatchQueue, DispatchRetained};
use std::sync::OnceLock;

static CAPTURE_QUEUE: OnceLock<DispatchRetained<DispatchQueue>> = OnceLock::new();

/// 返回专用的帧捕获串行队列（全局单例，惰性初始化）。
///
/// 使用串行队列保证帧回调顺序执行，且不阻塞主线程。
pub fn capture_queue() -> &'static DispatchQueue {
    CAPTURE_QUEUE
        .get_or_init(|| {
            // attr = None → DISPATCH_QUEUE_SERIAL（串行队列）
            DispatchQueue::new("com.rustcv.capture", None)
        })
        .as_ref()
}
