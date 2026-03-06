// 实现 AVCaptureVideoDataOutputSampleBufferDelegate，在帧回调中提取像素数据
// 并通过 tokio mpsc 通道将其发送给 Rust 消费者。
//
// 关键设计决策：
//   - 使用 `objc2::rc::autoreleasepool` 包裹每次回调，防止 Objective-C
//     自动释放对象在高频调用中堆积导致 OOM。
//   - AvfFrameData 携带 bytes_per_row（步长），Frame 构造时直接使用，
//     避免任何硬编码估算。
//   - CVPixelBuffer lock/unlock 严格配对，即使 base_addr 为 null 也会解锁。
//
//   IVar 初始化说明（objc2 0.6 正确模式）：
//   define_class! 的 #[ivars = T] 把 ivar 存储为 MaybeUninit<T>。
//   初始化必须通过 `Self::alloc().set_ivars(value)` 完成，
//   然后调用 `msg_send![super(this), init]`，才能让 `self.ivars()` 安全访问。
//   原来代码的 `msg_send![Self::class(), new]` 跳过了 set_ivars 步骤，
//   导致 "tried to access uninitialized instance variable" panic。

use objc2::rc::{autoreleasepool, Allocated};
use objc2::{define_class, msg_send, rc::Retained, AllocAnyThread, DeclaredClass};
use objc2_av_foundation::{
    AVCaptureConnection, AVCaptureOutput, AVCaptureVideoDataOutputSampleBufferDelegate,
};
use objc2_core_media::CMSampleBuffer;
use objc2_core_video::{
    CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow, CVPixelBufferGetDataSize,
    CVPixelBufferGetHeight, CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress,
    CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
};
use objc2_foundation::{NSObject, NSObjectProtocol};
use tokio::sync::mpsc::UnboundedSender;

// ---------------------------------------------------------------------------
// 帧数据包：携带从 CVPixelBuffer 中读取的所有信息
// ---------------------------------------------------------------------------

/// 从单帧回调中提取的原始帧数据。
pub struct AvfFrameData {
    /// 像素数据（深拷贝自 CVPixelBuffer）
    pub data: Vec<u8>,
    /// 图像宽度（像素）
    pub width: usize,
    /// 图像高度（像素）
    pub height: usize,
    /// 每行字节数（步长 / stride / bytes_per_row）
    pub bytes_per_row: usize,
    /// 硬件时间戳（CMTime 转换为纳秒，单调递增）
    pub timestamp_ns: u64,
}

// ---------------------------------------------------------------------------
// Ivar 包装类型
// ---------------------------------------------------------------------------

/// Delegate 内部持有的状态。
/// 使用独立结构体，避免在 ivar 中直接使用泛型类型。
pub struct DelegateIvars {
    pub sender: UnboundedSender<AvfFrameData>,
}

// ---------------------------------------------------------------------------
// Objective-C Delegate 类定义
// ---------------------------------------------------------------------------

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "RustCVCaptureDelegate"]
    #[ivars = DelegateIvars]
    pub struct CaptureDelegate;

    impl CaptureDelegate {
        /// AVCaptureVideoDataOutputSampleBufferDelegate 回调
        /// 每当新帧就绪时，由 GCD 串行队列在后台线程调用此方法。
        #[unsafe(method(captureOutput:didOutputSampleBuffer:fromConnection:))]
        unsafe fn capture_output(
            &self,
            _output: &AVCaptureOutput,
            sample_buffer: &CMSampleBuffer,
            _connection: &AVCaptureConnection,
        ) {
            let sender = &self.ivars().sender;

            // 1. 提取时间戳（CMSampleBufferGetPresentationTimeStamp）
            //    CMTime { value, timescale } → 纳秒 = value * 1_000_000_000 / timescale
            let timestamp_ns = {
                let pts = sample_buffer.presentation_time_stamp();
                if pts.timescale > 0 {
                    (pts.value as u64)
                        .wrapping_mul(1_000_000_000)
                        .wrapping_div(pts.timescale as u64)
                } else {
                    0
                }
            };

            // 2. 将整个像素提取包裹在 autorelease pool 中，防止 ObjC 临时对象堆积
            autoreleasepool(|_pool| {
                // 3. 从 CMSampleBuffer 获取 CVImageBuffer（即 CVPixelBuffer）
                let image_buffer_opt = unsafe { sample_buffer.image_buffer() };
                let Some(image_buffer) = image_buffer_opt else {
                    return;
                };

                let pixel_buffer = &*image_buffer;

                unsafe {
                    // 4. 锁定 CVPixelBuffer 内存（只读）
                    CVPixelBufferLockBaseAddress(pixel_buffer, CVPixelBufferLockFlags::ReadOnly);

                    let base_addr = CVPixelBufferGetBaseAddress(pixel_buffer) as *const u8;
                    let size = CVPixelBufferGetDataSize(pixel_buffer);
                    let width = CVPixelBufferGetWidth(pixel_buffer);
                    let height = CVPixelBufferGetHeight(pixel_buffer);
                    let bytes_per_row = CVPixelBufferGetBytesPerRow(pixel_buffer);

                    if !base_addr.is_null() && size > 0 {
                        // 5. 深拷贝像素数据到 Vec<u8>（必要的：缓冲区归 AVFoundation 所有）
                        let slice = std::slice::from_raw_parts(base_addr, size);
                        let frame = AvfFrameData {
                            data: slice.to_vec(),
                            width,
                            height,
                            bytes_per_row,
                            timestamp_ns,
                        };

                        // 6. 非阻塞发送（若接收端已关闭则静默丢弃）
                        let _ = sender.send(frame);
                    }

                    // 7. 解锁（必须与 Lock 严格配对，无论数据是否有效）
                    CVPixelBufferUnlockBaseAddress(pixel_buffer, CVPixelBufferLockFlags::ReadOnly);
                }
            });
        }
    }
);

// ---------------------------------------------------------------------------
// 协议声明
// ---------------------------------------------------------------------------

unsafe impl NSObjectProtocol for CaptureDelegate {}
unsafe impl AVCaptureVideoDataOutputSampleBufferDelegate for CaptureDelegate {}

// ---------------------------------------------------------------------------
// 构造函数
// ---------------------------------------------------------------------------

impl CaptureDelegate {
    /// 创建新的 Delegate 实例并绑定 Sender。
    ///
    /// # objc2 0.6 IVar 初始化正确模式
    ///
    /// ```text
    ///   Self::alloc()          → 分配内存（Allocated<Self>，ivar 为 MaybeUninit）
    ///   .set_ivars(value)      → 写入初始值（将 MaybeUninit 标记为已初始化）
    ///   → msg_send![super(this), init]  → 调用父类 init，返回 Retained<Self>
    /// ```
    ///
    /// 这是 objc2 0.6 文档中唯一正确的带 ivar 初始化的对象构造方式。
    /// 不能使用 `msg_send![Self::class(), new]`，因为 `new` = alloc+init 但
    /// 绕过了 `set_ivars`，导致 self.ivars() 访问 MaybeUninit 而 panic。
    pub fn new(sender: UnboundedSender<AvfFrameData>) -> Retained<Self> {
        // Step 1: 分配内存并写入 ivar 初始值
        let this: Allocated<Self> = Self::alloc();
        let this = this.set_ivars(DelegateIvars { sender });

        // Step 2: 调用父类（NSObject）的 init，转换为完整的 Retained<Self>
        unsafe { msg_send![super(this), init] }
    }
}
