/// macOS AVFoundation camera backend.
/// macOS AVFoundation 摄像头后端。
///
/// Wraps the thin Objective-C bridge (`bridge.m`) via FFI.
/// 通过 FFI 封装薄 Objective-C 桥接层（`bridge.m`）。
///
/// ## Architecture
///
/// AVFoundation is push-based (callback-driven), while our `Backend` API is
/// pull-based (`dequeue()` blocks until a frame is ready).  The bridge resolves
/// this mismatch with a `pthread_mutex + pthread_cond` pair:
///
/// - The AVFoundation capture thread calls the ObjC delegate, which copies the
///   BGRA32 pixel data into an internal buffer and signals the condvar.
/// - `dequeue()` waits on the condvar, then copies data into `self.frame_buf`
///   (a `Vec<u8>` owned by `AvfBackend`).
/// - The returned `RawFrame<'_>` borrows from `self.frame_buf`, tying its
///   lifetime to `&mut self` — the same safety contract as the V4L2 backend.
///
/// ## 架构
///
/// AVFoundation 是推送模型（回调驱动），而我们的后端 API 是拉取模型
/// （`dequeue()` 阻塞直到帧就绪）。桥接层通过 `pthread_mutex + pthread_cond`
/// 解决这个不匹配：
///
/// - AVFoundation 采集线程调用 ObjC delegate，将 BGRA32 像素数据拷贝到
///   内部缓冲区并发出 condvar 信号。
/// - `dequeue()` 等待 condvar，然后将数据拷贝到 `self.frame_buf`（`AvfBackend`
///   拥有的 `Vec<u8>`）。
/// - 返回的 `RawFrame<'_>` 借用 `self.frame_buf`，将其生命周期绑定到
///   `&mut self` —— 与 V4L2 后端相同的安全契约。
use std::os::raw::{c_int, c_uint};

use crate::config::{CameraConfig, ResolvedConfig};
use crate::error::{CameraError, Result};
use crate::pixel_format::PixelFormat;

use super::RawFrame;

// ─── FFI declarations ────────────────────────────────────────────────────────

mod sys {
    use std::os::raw::{c_int, c_uint};

    /// Opaque handle to the ObjC camera session.
    /// ObjC 摄像头会话的不透明句柄。
    #[repr(C)]
    pub struct AvfCameraOpaque {
        _private: [u8; 0],
    }

    extern "C" {
        pub fn avf_camera_open(
            index: c_uint,
            width: c_uint,
            height: c_uint,
            fps: c_uint,
            actual_width: *mut c_uint,
            actual_height: *mut c_uint,
            out_cam: *mut *mut AvfCameraOpaque,
        ) -> c_int;

        pub fn avf_camera_start(cam: *mut AvfCameraOpaque);

        pub fn avf_camera_stop(cam: *mut AvfCameraOpaque);

        pub fn avf_camera_dequeue_blocking(
            cam: *mut AvfCameraOpaque,
            buf: *mut u8,
            buf_cap: c_uint,
            out_len: *mut c_uint,
            out_width: *mut c_uint,
            out_height: *mut c_uint,
            out_seq: *mut u64,
            out_ts_us: *mut u64,
        ) -> c_int;

        pub fn avf_camera_free(cam: *mut AvfCameraOpaque);
    }
}

const AVF_OK: c_int = 0;
const AVF_ERR_PERMISSION: c_int = -1;
const AVF_ERR_DEVICE_NOT_FOUND: c_int = -2;
const AVF_ERR_STOPPED: c_int = -4;

// ─── AvfBackend ──────────────────────────────────────────────────────────────

/// macOS AVFoundation camera backend.
/// macOS AVFoundation 摄像头后端。
pub(crate) struct AvfBackend {
    /// Opaque handle to the ObjC session.  `null` before `open()`.
    /// ObjC 会话的不透明句柄。`open()` 前为 null。
    cam: *mut sys::AvfCameraOpaque,

    /// Owned buffer for the current frame's BGRA32 pixel data.
    /// Borrowed by the `RawFrame<'_>` returned from `dequeue()`.
    ///
    /// 当前帧 BGRA32 像素数据的所有缓冲区。
    /// 由 `dequeue()` 返回的 `RawFrame<'_>` 借用。
    frame_buf: Vec<u8>,
}

/// # Safety
/// `cam` is an opaque C pointer accessed only from one thread at a time.
/// `cam` 是一个不透明的 C 指针，同一时间只从一个线程访问。
unsafe impl Send for AvfBackend {}

impl AvfBackend {
    /// Create a new (uninitialized) AVFoundation backend.
    /// 创建新的（未初始化的）AVFoundation 后端。
    pub fn new() -> Self {
        Self {
            cam: std::ptr::null_mut(),
            frame_buf: Vec::new(),
        }
    }

    /// Open the camera device at `index` (embedded in the `device` string).
    /// 打开 `device` 字符串中 `index` 指定的摄像头设备。
    pub fn open(&mut self, device: &str, config: &CameraConfig) -> Result<ResolvedConfig> {
        // On macOS the "device path" is just the numeric index as a string.
        // 在 macOS 上，"设备路径"只是数字索引的字符串形式。
        let index: c_uint = device.parse().unwrap_or(0);
        let width = config.width.unwrap_or(640) as c_uint;
        let height = config.height.unwrap_or(480) as c_uint;
        let fps = config.fps.unwrap_or(30) as c_uint;

        let mut actual_w: c_uint = width;
        let mut actual_h: c_uint = height;
        let mut cam_ptr: *mut sys::AvfCameraOpaque = std::ptr::null_mut();

        let ret = unsafe {
            sys::avf_camera_open(
                index,
                width,
                height,
                fps,
                &mut actual_w,
                &mut actual_h,
                &mut cam_ptr,
            )
        };

        match ret {
            AVF_OK => {}
            AVF_ERR_PERMISSION => {
                return Err(CameraError::DeviceNotFound(
                    "camera access denied — allow camera in System Settings > Privacy".to_string(),
                ));
            }
            AVF_ERR_DEVICE_NOT_FOUND => {
                return Err(CameraError::DeviceNotFound(format!(
                    "no camera at index {}",
                    index
                )));
            }
            code => {
                return Err(CameraError::DeviceNotFound(format!(
                    "AVFoundation session error (code {})",
                    code
                )));
            }
        }

        self.cam = cam_ptr;

        // Pre-allocate the frame buffer for the requested resolution (BGRA32).
        // The bridge (bridge.m) guarantees frames are delivered at exactly
        // (actual_w × actual_h) via vImage scaling, so no headroom is needed.
        // 按请求分辨率预分配帧缓冲区（BGRA32）。
        // bridge 通过 vImage 缩放保证每帧都以 (actual_w × actual_h) 交付，
        // 无需额外预留空间。
        self.frame_buf.resize((actual_w * actual_h * 4) as usize, 0);

        Ok(ResolvedConfig {
            width: actual_w,
            height: actual_h,
            fps: config.fps.unwrap_or(30),
            pixel_format: PixelFormat::Bgra32,
            buffer_count: 1, // AVFoundation manages its own internal ring
        })
    }

    /// Start the capture session.
    /// 启动采集会话。
    pub fn start(&mut self) -> Result<()> {
        unsafe { sys::avf_camera_start(self.cam) };
        Ok(())
    }

    /// Stop the capture session.
    /// 停止采集会话。
    pub fn stop(&mut self) -> Result<()> {
        if !self.cam.is_null() {
            unsafe { sys::avf_camera_stop(self.cam) };
        }
        Ok(())
    }

    /// Dequeue the next frame (blocking).
    /// 出队下一帧（阻塞）。
    ///
    /// Blocks until AVFoundation delivers a frame, then copies BGRA32 pixel data
    /// into `self.frame_buf`. Returns a `RawFrame<'_>` borrowing that buffer.
    ///
    /// 阻塞直到 AVFoundation 投递一帧，然后将 BGRA32 像素数据拷贝到
    /// `self.frame_buf`。返回借用该缓冲区的 `RawFrame<'_>`。
    pub fn dequeue(&mut self) -> Result<RawFrame<'_>> {
        let buf_cap = self.frame_buf.len() as c_uint;
        let mut out_len: c_uint = 0;
        let mut out_w: c_uint = 0;
        let mut out_h: c_uint = 0;
        let mut out_seq: u64 = 0;
        let mut out_ts: u64 = 0;

        let ret = unsafe {
            sys::avf_camera_dequeue_blocking(
                self.cam,
                self.frame_buf.as_mut_ptr(),
                buf_cap,
                &mut out_len,
                &mut out_w,
                &mut out_h,
                &mut out_seq,
                &mut out_ts,
            )
        };

        if ret == AVF_ERR_STOPPED {
            return Err(CameraError::Io(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "camera session stopped",
            )));
        }
        if ret != AVF_OK {
            return Err(CameraError::Io(std::io::Error::other(format!(
                "dequeue error (code {})",
                ret
            ))));
        }

        // If the camera reported dimensions larger than our buffer, grow it for
        // the next frame.  This frame's data is already truncated but avoids UB.
        // 如果摄像头报告的尺寸大于缓冲区，为下一帧扩容。
        // 本帧数据已截断，但避免了未定义行为。
        let expected = (out_w * out_h * 4) as usize;
        if self.frame_buf.len() < expected {
            self.frame_buf.resize(expected, 0);
        }

        Ok(RawFrame {
            index: 0, // AVFoundation manages buffers internally
            data: &self.frame_buf[..out_len as usize],
            width: out_w,
            height: out_h,
            pixel_format: PixelFormat::Bgra32,
            sequence: out_seq,
            timestamp_us: out_ts,
        })
    }
}

impl Drop for AvfBackend {
    fn drop(&mut self) {
        if !self.cam.is_null() {
            let _ = self.stop();
            unsafe { sys::avf_camera_free(self.cam) };
            self.cam = std::ptr::null_mut();
        }
    }
}
