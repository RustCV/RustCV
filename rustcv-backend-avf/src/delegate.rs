// src/delegate.rs
use objc2::{ClassType, DeclaredClass, define_class, msg_send, rc::Retained};
use objc2_av_foundation::{
    AVCaptureConnection, AVCaptureOutput, AVCaptureVideoDataOutputSampleBufferDelegate,
};
use objc2_core_media::CMSampleBuffer;
use objc2_core_video::{
    CVPixelBufferGetBaseAddress, CVPixelBufferGetDataSize, CVPixelBufferGetHeight,
    CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags,
    CVPixelBufferUnlockBaseAddress,
};
use objc2_foundation::{NSObject, NSObjectProtocol};
use std::sync::OnceLock;
use tokio::sync::mpsc::UnboundedSender;

// 定义数据包结构
pub struct AvfFrameData {
    pub data: Vec<u8>,
    pub width: usize,
    pub height: usize,
}

// 使用新的 define_class! 宏
define_class!(
    #[unsafe(super(NSObject))]
    #[name = "RustCVCaptureDelegate"]
    // 使用 OnceLock 来存储 Sender，确保线程安全初始化
    #[ivars = OnceLock<UnboundedSender<AvfFrameData>>]
    pub struct CaptureDelegate;

    // 方法实现写在 impl 块中
    impl CaptureDelegate {
        #[unsafe(method(captureOutput:didOutputSampleBuffer:fromConnection:))]
        unsafe fn capture_output(
            &self,
            _output: &AVCaptureOutput,
            sample_buffer: &CMSampleBuffer,
            _connection: &AVCaptureConnection,
        ) {
            // 1. 获取 Sender
            // ivars() 返回的是 &OnceLock<...>
            let sender = match self.ivars().get() {
                Some(s) => s,
                None => return, // 如果未初始化，直接忽略
            };

            // 2. 从 SampleBuffer 提取图像
            let image_buffer_opt = unsafe { sample_buffer.image_buffer() };
            if let Some(image_buffer) = image_buffer_opt {
                // 3. 锁定内存
                // image_buffer 是 Retained<CVPixelBuffer>，可以像引用一样使用
                let pixel_buffer = &*image_buffer;

                unsafe {
                    CVPixelBufferLockBaseAddress(pixel_buffer, CVPixelBufferLockFlags::ReadOnly);

                    let base_addr = CVPixelBufferGetBaseAddress(pixel_buffer) as *const u8;
                    let size = CVPixelBufferGetDataSize(pixel_buffer);
                    let w = CVPixelBufferGetWidth(pixel_buffer);
                    let h = CVPixelBufferGetHeight(pixel_buffer);

                    if !base_addr.is_null() && size > 0 {
                        // 4. 深拷贝数据
                        let slice = std::slice::from_raw_parts(base_addr, size);
                        let frame = AvfFrameData {
                            data: slice.to_vec(),
                            width: w,
                            height: h,
                        };

                        // 5. 发送给 Rust Stream (非阻塞)
                        let _ = sender.send(frame);
                    }
                }

                // 6. 解锁内存
                unsafe {
                    CVPixelBufferUnlockBaseAddress(pixel_buffer, CVPixelBufferLockFlags::ReadOnly);
                }
            }
        }
    }
);

// 显式声明实现的协议
unsafe impl NSObjectProtocol for CaptureDelegate {}
unsafe impl AVCaptureVideoDataOutputSampleBufferDelegate for CaptureDelegate {}

// 构造函数
impl CaptureDelegate {
    pub fn new(sender: UnboundedSender<AvfFrameData>) -> Retained<Self> {
        unsafe {
            // 创建对象
            let obj: Retained<Self> = msg_send![Self::class(), new];
            // 初始化 ivar
            let _ = obj.ivars().set(sender);
            obj
        }
    }
}
