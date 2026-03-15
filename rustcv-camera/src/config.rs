/// Camera configuration builder.
/// 摄像头配置构建器。
///
/// Uses the Builder pattern for ergonomic, chainable configuration.
/// All fields are optional — the backend will negotiate sensible defaults.
/// 采用 Builder 模式实现链式配置。
/// 所有字段均为可选 —— 后端会自动协商合理的默认值。
use crate::pixel_format::PixelFormat;

/// Configuration for opening a camera device.
/// 打开摄像头设备的配置。
///
/// # Examples
///
/// ```no_run
/// use rustcv_camera::CameraConfig;
///
/// let config = CameraConfig::new()
///     .resolution(640, 480)
///     .fps(30);
/// ```
#[derive(Debug, Clone)]
pub struct CameraConfig {
    /// Requested width in pixels. `None` = let the driver choose.
    /// 请求的宽度（像素）。`None` = 由驱动自行选择。
    pub(crate) width: Option<u32>,

    /// Requested height in pixels. `None` = let the driver choose.
    /// 请求的高度（像素）。`None` = 由驱动自行选择。
    pub(crate) height: Option<u32>,

    /// Requested frames per second. `None` = driver default (usually 30).
    /// 请求的帧率。`None` = 驱动默认值（通常为 30）。
    pub(crate) fps: Option<u32>,

    /// Requested pixel format. `None` = auto-select based on fps.
    /// 请求的像素格式。`None` = 根据帧率自动选择。
    ///
    /// Auto-selection strategy:
    /// - fps < 60: prefer MJPEG (lower USB bandwidth)
    /// - fps >= 60: prefer YUYV/NV12 (lower decode overhead)
    ///
    /// 自动选择策略：
    /// - fps < 60：优先 MJPEG（USB 带宽更低）
    /// - fps >= 60：优先 YUYV/NV12（解码开销更小）
    pub(crate) pixel_format: Option<PixelFormat>,

    /// Number of kernel mmap buffers to allocate.
    /// 要分配的内核 mmap 缓冲区数量。
    ///
    /// More buffers = more tolerance for processing jitter,
    /// but uses more kernel memory.
    /// Default is 5, which provides ~166ms of buffer at 30fps.
    ///
    /// 缓冲区越多 = 对处理抖动的容忍度越高，但消耗更多内核内存。
    /// 默认为 5，在 30fps 下提供约 166ms 的缓冲余量。
    pub(crate) buffer_count: u32,
}

impl Default for CameraConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl CameraConfig {
    /// Create a new configuration with sensible defaults.
    /// 使用合理的默认值创建新配置。
    pub fn new() -> Self {
        Self {
            width: None,
            height: None,
            fps: None,
            pixel_format: None,
            buffer_count: 5,
        }
    }

    /// Set the desired resolution.
    /// 设置期望的分辨率。
    pub fn resolution(mut self, width: u32, height: u32) -> Self {
        self.width = Some(width);
        self.height = Some(height);
        self
    }

    /// Set the desired frame rate.
    /// 设置期望的帧率。
    pub fn fps(mut self, fps: u32) -> Self {
        self.fps = Some(fps);
        self
    }

    /// Set the desired pixel format.
    /// 设置期望的像素格式。
    ///
    /// If not set, the format is auto-selected based on the requested fps.
    /// 如果未设置，将根据请求的帧率自动选择格式。
    pub fn pixel_format(mut self, format: PixelFormat) -> Self {
        self.pixel_format = Some(format);
        self
    }

    /// Set the number of mmap buffers to allocate.
    /// 设置要分配的 mmap 缓冲区数量。
    ///
    /// Recommended values by target fps:
    /// - 30 fps: 5 buffers (default)
    /// - 60 fps: 8 buffers
    /// - 120 fps: 12 buffers
    ///
    /// 根据目标帧率的推荐值：
    /// - 30 fps：5 个缓冲区（默认）
    /// - 60 fps：8 个缓冲区
    /// - 120 fps：12 个缓冲区
    pub fn buffer_count(mut self, count: u32) -> Self {
        self.buffer_count = count;
        self
    }
}

/// The actual configuration negotiated with the device driver.
/// 与设备驱动协商后的实际配置。
///
/// The driver may adjust the requested values to the closest supported ones.
/// This struct holds what was actually applied.
/// 驱动可能会将请求值调整为最接近的支持值。此结构体保存实际应用的配置。
#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    /// Actual width the driver applied.
    /// 驱动实际应用的宽度。
    pub width: u32,

    /// Actual height the driver applied.
    /// 驱动实际应用的高度。
    pub height: u32,

    /// Actual frame rate the driver applied.
    /// 驱动实际应用的帧率。
    pub fps: u32,

    /// Actual pixel format the driver applied.
    /// 驱动实际应用的像素格式。
    pub pixel_format: PixelFormat,

    /// Actual number of buffers allocated.
    /// 实际分配的缓冲区数量。
    pub buffer_count: u32,
}
