// AvfStream：macOS AVFoundation 摄像头采集管线。
//
// 设计要点：
//   • 输出格式固定为 32BGRA（kCMPixelFormat_32BGRA），避免 CPU YUV→RGB 转换；
//   • 使用专用串行 GCD 队列（非主队列），防止死锁；
//   • alwaysDiscardsLateVideoFrames = true，防止处理不及时时内存爆炸；
//   • AvfStream 实现 Drop，确保退出时调用 stopRunning()；
//   • 所有长期对象（session, input, output, delegate）以 Retained<T> 保存，
//     避免提前释放。

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use objc2::exception::catch;
use objc2_av_foundation::{
    AVCaptureDevice, AVCaptureDeviceFormat, AVCaptureDeviceInput, AVCaptureSession,
    AVCaptureVideoDataOutput,
};
use objc2_core_media::{CMTime, CMTimeFlags, CMVideoFormatDescriptionGetDimensions};
use objc2_foundation::{NSNumber, NSString};
use rustcv_core::builder::CameraConfig;
use rustcv_core::error::Result as CvResult;
use tokio::sync::mpsc::{channel, Receiver};

use rustcv_core::frame::{BackendBufferHandle, Frame, FrameMetadata, Timestamp};
use rustcv_core::pixel_format::FourCC;
use rustcv_core::traits::Stream;

use crate::delegate::{AvfFrameData, CaptureDelegate};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;

// ---------------------------------------------------------------------------
// 后端句柄（零大小类型，仅用于满足 Frame 的 BackendBufferHandle 约束）
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct AvfBufferHandle;
impl BackendBufferHandle for AvfBufferHandle {}
static AVF_HANDLE: AvfBufferHandle = AvfBufferHandle;

// kCVPixelFormatType_32BGRA = 'BGRA' = 0x42_47_52_41 = 1_111_970_369
// FourCC: 'B'=0x42, 'G'=0x47, 'R'=0x52, 'A'=0x41  (big-endian u32)
// 错误旧值 875_704_422 = 0x34_32_30_66 = '420f' = NV12 双平面 YUV！
const PIXEL_FORMAT_BGRA: u32 = 1_111_970_369;

// ---------------------------------------------------------------------------
// AvfStream
// ---------------------------------------------------------------------------

pub struct AvfStream {
    /// 核心采集会话（必须持有以维持引用计数）
    session: Retained<AVCaptureSession>,
    /// Delegate（必须持有）
    _delegate: Retained<CaptureDelegate>,
    /// 输入设备（必须持有）
    _input: Retained<AVCaptureDeviceInput>,
    /// 输出（必须持有）
    _output: Retained<AVCaptureVideoDataOutput>,

    /// 捕获设备（用于 activeFormat 配置锁）
    device: Retained<AVCaptureDevice>,

    /// 帧数据接收端
    receiver: Receiver<AvfFrameData>,
    /// 当前帧（借用给 Frame<'_> 时使用）
    current_frame: Option<AvfFrameData>,

    /// 采集状态标志
    is_streaming: bool,
    /// activeFormat 已设置且设备配置锁仍被持有。
    /// 必须保持到 startRunning 之后，否则会话会把设备恢复为默认格式
    /// （某些 UVC 设备的默认格式为竖屏）。
    config_lock_held: bool,
}

/// # Safety
/// AVCapture 对象由 AVFoundation 内部线程安全地管理；
/// 我们在 Rust 侧保证同时只有一个线程访问此结构体（通过 &mut self），
/// 因此标记 Send 是安全的。
unsafe impl Send for AvfStream {}

impl AvfStream {
    /// 初始化完整的采集管线（同步，不启动采集）。
    ///
    /// # Arguments
    /// * `device_id` — 由 `AvfDriver::list_devices` 返回的 `DeviceInfo.id`
    /// * `config` — 采集配置（分辨率、FPS 等）
    pub fn new(device_id: &str, config: CameraConfig) -> Result<Self> {
        unsafe {
            // 创建 Session
            let session = AVCaptureSession::new();

            // 开始修改配置
            session.beginConfiguration();

            // 查找设备
            let device = AVCaptureDevice::deviceWithUniqueID(&NSString::from_str(device_id))
                .ok_or_else(|| anyhow!("Device ID not found: {}", device_id))?;

            // ⑥ 包装为 Input 并添加到 Session
            let input = AVCaptureDeviceInput::deviceInputWithDevice_error(&device)
                .map_err(|e| anyhow!("Failed to create capture input: {:?}", e))?;

            if session.canAddInput(&input) {
                session.addInput(&input);
            } else {
                session.commitConfiguration();
                return Err(anyhow!("Cannot add input to session"));
            }

            // 优先通过 activeFormat 精确选择设备格式（分辨率完全匹配时）。
            //
            // 仅靠 sessionPreset 不可靠：某些设备（如竖屏优先的 UVC 相机）即使
            // canSetSessionPreset 返回 true，会话仍会选中设备的默认（竖屏）格式。
            // 设置 activeFormat 后必须持有配置锁直到 startRunning 之后，否则
            // commitConfiguration/startRunning 会把设备重置回默认格式。
            let mut config_lock_held = false;
            if let Some(format) = select_active_format(&device, &config) {
                if device.lockForConfiguration().is_ok() {
                    let device_ref = std::panic::AssertUnwindSafe(&device);
                    let format_ref = std::panic::AssertUnwindSafe(&format);
                    let requested_fps = config.fps_req.map(|(fps, _)| fps);
                    let result = catch(|| {
                        device_ref.setActiveFormat(&format_ref);
                        // FPS 必须从该格式支持的范围中选取，否则抛出异常
                        if let Some(duration) =
                            best_frame_duration_for_format(&format_ref, requested_fps)
                        {
                            device_ref.setActiveVideoMinFrameDuration(duration);
                            device_ref.setActiveVideoMaxFrameDuration(duration);
                        }
                    });
                    match result {
                        Ok(()) => config_lock_held = true,
                        Err(e) => {
                            println!("Warning: Failed to set activeFormat. Exception: {:?}", e);
                            device.unlockForConfiguration();
                        }
                    }
                }
            }

            if !config_lock_held {
                // 回退：根据 config 选择最合适的分辨率 Preset
                let preset = select_best_preset(&session, &config);
                session.setSessionPreset(preset);

                // 配置 FPS (用 catch 包裹，因为如果申请了设备不支持的 FPS 会抛出 Objective-C 异常)
                if let Some((fps, _priority)) = config.fps_req {
                    if device.lockForConfiguration().is_ok() {
                        let device_ref = std::panic::AssertUnwindSafe(&device);
                        let result = catch(|| {
                            let duration = CMTime {
                                value: 1,
                                timescale: fps as i32,
                                flags: CMTimeFlags::Valid,
                                epoch: 0,
                            };
                            device_ref.setActiveVideoMinFrameDuration(duration);
                            device_ref.setActiveVideoMaxFrameDuration(duration);
                        });
                        if let Err(e) = result {
                            println!("Warning: Failed to set FPS to {fps}. Device might not support it at this resolution. Exception: {:?}", e);
                        }
                        device.unlockForConfiguration();
                    }
                }
            }

            // ⑥ 创建 Output，并配置像素格式和丢帧策略
            let output = AVCaptureVideoDataOutput::new();

            // 强制请求 32BGRA，避免后续 CPU YUV→RGB 转换的高昂开销
            // kCVPixelBufferPixelFormatTypeKey = "PixelFormatType"
            {
                use objc2::runtime::AnyObject;
                use objc2_foundation::{NSCopying, NSDictionary, NSObjectProtocol};

                let key = NSString::from_str("PixelFormatType");
                let val = NSNumber::new_u32(PIXEL_FORMAT_BGRA);

                let key_proto: &ProtocolObject<dyn NSCopying> = std::mem::transmute(&*key);
                let val_proto: &AnyObject =
                    std::mem::transmute(ProtocolObject::<dyn NSObjectProtocol>::from_ref(&*val));

                let settings = NSDictionary::<NSString, AnyObject>::dictionaryWithObject_forKey(
                    val_proto, key_proto,
                );
                output.setVideoSettings(Some(&settings));
            }

            // 关键：处理不过来时丢弃最新帧，防止帧队列无限增长 → OOM
            output.setAlwaysDiscardsLateVideoFrames(true);

            // ⑦ 绑定 Delegate 和 GCD 串行队列
            let (tx, rx) = channel::<AvfFrameData>(2);
            let delegate = CaptureDelegate::new(tx);
            let queue = crate::gcd::capture_queue();

            let delegate_proto = ProtocolObject::from_ref(&*delegate);
            output.setSampleBufferDelegate_queue(Some(delegate_proto), Some(queue));

            // ⑧ 添加 Output 到 Session
            if session.canAddOutput(&output) {
                session.addOutput(&output);
            } else {
                session.commitConfiguration();
                return Err(anyhow!("Cannot add output to session"));
            }

            // ⑨ 提交配置（与 beginConfiguration 配对）
            session.commitConfiguration();

            Ok(Self {
                session,
                _delegate: delegate,
                _input: input,
                _output: output,
                device,
                receiver: rx,
                current_frame: None,
                is_streaming: false,
                config_lock_held,
            })
        }
    }

    /// 若仍持有设备配置锁则释放（在 startRunning 之后调用才安全）。
    fn release_config_lock(&mut self) {
        if self.config_lock_held {
            unsafe {
                self.device.unlockForConfiguration();
            }
            self.config_lock_held = false;
        }
    }
}

// ---------------------------------------------------------------------------
// Drop：确保销毁时停止采集
// ---------------------------------------------------------------------------

impl Drop for AvfStream {
    fn drop(&mut self) {
        if self.is_streaming {
            unsafe {
                self.session.stopRunning();
            }
            self.is_streaming = false;
        }
        self.release_config_lock();
    }
}

// ---------------------------------------------------------------------------
// Stream trait 实现
// ---------------------------------------------------------------------------

#[async_trait]
impl Stream for AvfStream {
    /// 启动帧采集。
    async fn start(&mut self) -> CvResult<()> {
        unsafe {
            self.session.startRunning();
        }
        // activeFormat 的配置锁必须保持到 startRunning 之后才能释放，
        // 否则会话启动时会把设备重置回默认格式。
        self.release_config_lock();
        self.is_streaming = true;
        Ok(())
    }

    /// 停止帧采集（不销毁管线，可重新 start）。
    async fn stop(&mut self) -> CvResult<()> {
        unsafe {
            self.session.stopRunning();
        }
        self.is_streaming = false;
        Ok(())
    }

    /// 等待并返回下一帧。
    ///
    /// 帧数据的生命周期绑定到 `self`（零拷贝借用语义）。
    async fn next_frame(&mut self) -> CvResult<Frame<'_>> {
        use rustcv_core::error::CameraError;

        if !self.is_streaming {
            return Err(CameraError::Io(std::io::Error::other(
                "Stream not started — call start() first",
            )));
        }

        // 阻塞等待直到 delegate 推送一帧（或 channel 关闭）
        let frame_data = self.receiver.recv().await.ok_or_else(|| {
            CameraError::Io(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "Capture channel closed",
            ))
        })?;

        self.current_frame = Some(frame_data);
        let f = self.current_frame.as_ref().unwrap();

        Ok(Frame {
            data: &f.data,
            width: f.width as u32,
            height: f.height as u32,
            // 使用 CVPixelBuffer 报告的真实步长，而非估算值
            stride: f.bytes_per_row,
            // 像素格式与上面请求的 BGRA 对应
            format: FourCC::new(b'B', b'G', b'R', b'A').into(),
            sequence: 0,
            timestamp: Timestamp {
                hw_raw_ns: f.timestamp_ns,
                system_synced: std::time::Duration::ZERO,
            },
            metadata: FrameMetadata::default(),
            backend_handle: &AVF_HANDLE,
        })
    }

    #[cfg(feature = "simulation")]
    async fn inject_frame(&mut self, _frame: Frame<'_>) -> CvResult<()> {
        Ok(())
    }
}

/// 在设备支持的格式中查找与请求分辨率完全匹配的格式（按优先级从高到低）。
fn select_active_format(
    device: &AVCaptureDevice,
    config: &CameraConfig,
) -> Option<Retained<AVCaptureDeviceFormat>> {
    if config.resolution_req.is_empty() {
        return None;
    }

    let mut reqs = config.resolution_req.clone();
    reqs.sort_by_key(|&(_, _, p)| std::cmp::Reverse(p));

    let formats = unsafe { device.formats() };
    for (w, h, _) in reqs {
        for format in &formats {
            let desc = unsafe { format.formatDescription() };
            let dims = unsafe { CMVideoFormatDescriptionGetDimensions(&desc) };
            if dims.width == w as i32 && dims.height == h as i32 {
                return Some(format);
            }
        }
    }
    None
}

/// 从格式支持的帧率范围中选取帧时长：优先匹配请求的 FPS，否则取最高帧率。
fn best_frame_duration_for_format(
    format: &AVCaptureDeviceFormat,
    requested_fps: Option<u32>,
) -> Option<CMTime> {
    let ranges = unsafe { format.videoSupportedFrameRateRanges() };
    let mut best: Option<(f64, CMTime)> = None;
    for range in &ranges {
        let max_rate = unsafe { range.maxFrameRate() };
        let duration = unsafe { range.minFrameDuration() };
        let score = match requested_fps {
            // 距离请求 FPS 越近越好（负距离，便于统一取最大值）
            Some(fps) => -(max_rate - fps as f64).abs(),
            None => max_rate,
        };
        if best.map_or(true, |(s, _)| score > s) {
            best = Some((score, duration));
        }
    }
    best.map(|(_, duration)| duration)
}

/// 根据用户配置选择最佳的 AVFoundation 会话预设。
fn select_best_preset(
    session: &AVCaptureSession,
    config: &CameraConfig,
) -> &'static objc2_av_foundation::AVCaptureSessionPreset {
    use objc2_av_foundation::*;

    // 如果没有要求，默认使用 High
    if config.resolution_req.is_empty() {
        return unsafe { AVCaptureSessionPresetHigh };
    }

    // 按优先级排序，取最高优先级的要求
    let mut reqs = config.resolution_req.clone();
    reqs.sort_by_key(|&(_, _, p)| std::cmp::Reverse(p));

    for (w, h, _) in reqs {
        let preset_opt = match (w, h) {
            (3840, 2160) => Some(unsafe { AVCaptureSessionPreset3840x2160 }),
            (1920, 1080) => Some(unsafe { AVCaptureSessionPreset1920x1080 }),
            (1280, 720) => Some(unsafe { AVCaptureSessionPreset1280x720 }),
            (960, 540) => Some(unsafe { AVCaptureSessionPreset960x540 }),
            (640, 480) => Some(unsafe { AVCaptureSessionPreset640x480 }),
            (352, 288) => Some(unsafe { AVCaptureSessionPreset352x288 }),
            (320, 240) => Some(unsafe { AVCaptureSessionPreset320x240 }),
            _ => None,
        };

        if let Some(preset) = preset_opt {
            if unsafe { session.canSetSessionPreset(preset) } {
                return preset;
            }
        }
    }

    unsafe { AVCaptureSessionPresetHigh }
}
