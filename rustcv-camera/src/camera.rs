/// Rust-idiomatic camera API with zero-copy frame access.
/// Rust 惯用的摄像头 API，提供零拷贝帧访问。
///
/// This is the primary API for users who want maximum performance
/// and fine-grained control over frame lifecycle.
/// 这是面向追求最大性能和精细控制帧生命周期的用户的主要 API。
///
/// # Examples
///
/// ```no_run
/// use rustcv_camera::Camera;
///
/// let mut cam = Camera::open(0).unwrap();
/// for _ in 0..100 {
///     let frame = cam.next_frame().unwrap();
///     println!("{}x{} {:?} {} bytes",
///         frame.width(), frame.height(),
///         frame.pixel_format(), frame.data().len());
/// }
/// ```
use crate::backend;
use crate::config::{CameraConfig, ResolvedConfig};
use crate::error::Result;
use crate::frame::Frame;
use crate::mat::Mat;

/// A camera capture device with zero-copy frame access.
/// 具有零拷贝帧访问能力的摄像头采集设备。
///
/// `Camera` wraps a platform-specific backend and provides:
/// - [`next_frame()`](Self::next_frame): zero-copy frame borrowing mmap buffers
/// - [`read_decoded()`](Self::read_decoded): decode into a reusable Mat
/// - RAII buffer management (buffers auto-returned on frame drop)
///
/// `Camera` 封装平台特定的后端并提供：
/// - [`next_frame()`](Self::next_frame)：零拷贝帧，借用 mmap 缓冲区
/// - [`read_decoded()`](Self::read_decoded)：解码到可复用的 Mat
/// - RAII 缓冲区管理（帧 drop 时自动归还缓冲区）
pub struct Camera {
    /// Platform-specific backend (V4L2 on Linux).
    /// 平台特定后端（Linux 上为 V4L2）。
    backend: backend::PlatformBackend,

    /// The actual configuration negotiated with the driver.
    /// 与驱动协商后的实际配置。
    config: ResolvedConfig,
}

impl Camera {
    /// Open a camera by device index (0 = first camera).
    /// 按设备索引打开摄像头（0 = 第一个摄像头）。
    ///
    /// Uses default configuration: 640x480, 30fps, auto format selection.
    /// 使用默认配置：640x480，30fps，自动格式选择。
    pub fn open(index: u32) -> Result<Self> {
        Self::open_with(index, CameraConfig::new().resolution(640, 480).fps(30))
    }

    /// Open a camera with custom configuration.
    /// 使用自定义配置打开摄像头。
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use rustcv_camera::{Camera, CameraConfig, PixelFormat};
    ///
    /// let cam = Camera::open_with(0,
    ///     CameraConfig::new()
    ///         .resolution(1280, 720)
    ///         .fps(60)
    ///         .pixel_format(PixelFormat::Yuyv)
    ///         .buffer_count(8)
    /// ).unwrap();
    /// ```
    pub fn open_with(index: u32, config: CameraConfig) -> Result<Self> {
        let device_path = format!("/dev/video{}", index);

        let mut backend = backend::PlatformBackend::new();
        let resolved = backend.open(&device_path, &config)?;
        backend.start()?;

        Ok(Self {
            backend,
            config: resolved,
        })
    }

    /// Capture the next frame (zero-copy).
    /// 采集下一帧（零拷贝）。
    ///
    /// The returned [`Frame`] borrows directly from the kernel's mmap buffer.
    /// No data is copied. The frame must be dropped before calling
    /// `next_frame()` again (enforced by the borrow checker).
    ///
    /// 返回的 [`Frame`] 直接借用内核的 mmap 缓冲区。
    /// 不进行数据拷贝。帧必须在再次调用 `next_frame()` 前被 drop
    /// （由借用检查器强制保证）。
    ///
    /// # Blocking
    ///
    /// This call blocks until the next frame is ready from the camera.
    /// At 30fps this is ~33ms; at 120fps this is ~8.3ms.
    ///
    /// 此调用会阻塞直到摄像头的下一帧就绪。
    /// 30fps 时约 33ms；120fps 时约 8.3ms。
    pub fn next_frame(&mut self) -> Result<Frame<'_>> {
        let raw = self.backend.dequeue()?;
        Ok(Frame::new(
            raw.data,
            raw.width,
            raw.height,
            raw.pixel_format,
            raw.sequence,
            raw.timestamp_us,
        ))
    }

    /// Capture and decode the next frame into a reusable [`Mat`].
    /// 采集并解码下一帧到可复用的 [`Mat`]。
    ///
    /// This combines `next_frame()` + decode in a single call.
    /// The `Mat` is reused across calls to avoid per-frame allocation.
    ///
    /// 将 `next_frame()` + 解码合并为单次调用。
    /// `Mat` 跨调用复用以避免每帧分配。
    pub fn read_decoded(&mut self, mat: &mut Mat) -> Result<bool> {
        let frame = self.next_frame()?;
        crate::decode::decode_frame(&frame, mat, None)?;
        Ok(true)
    }

    /// Get the resolved (actual) configuration.
    /// 获取已协商的（实际）配置。
    pub fn config(&self) -> &ResolvedConfig {
        &self.config
    }

    /// Get the current image width.
    /// 获取当前图像宽度。
    pub fn width(&self) -> u32 {
        self.config.width
    }

    /// Get the current image height.
    /// 获取当前图像高度。
    pub fn height(&self) -> u32 {
        self.config.height
    }

    /// Get the current pixel format.
    /// 获取当前像素格式。
    pub fn pixel_format(&self) -> crate::pixel_format::PixelFormat {
        self.config.pixel_format
    }
}
