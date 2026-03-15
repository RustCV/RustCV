/// OpenCV-compatible camera API.
/// 兼容 OpenCV 的摄像头 API。
///
/// [`VideoCapture`] provides the familiar `cap.read(&mut frame)` pattern
/// that OpenCV users expect. Internally it wraps a [`Camera`] and
/// automatically decodes each frame to BGR.
///
/// [`VideoCapture`] 提供 OpenCV 用户熟悉的 `cap.read(&mut frame)` 模式。
/// 内部封装 [`Camera`] 并自动将每帧解码为 BGR。
///
/// # Examples
///
/// ```no_run
/// use rustcv_camera::{VideoCapture, Mat};
///
/// let mut cap = VideoCapture::open(0).unwrap();
/// let mut frame = Mat::new();
///
/// while cap.read(&mut frame).unwrap() {
///     println!("{}x{}", frame.cols(), frame.rows());
/// }
/// ```
use crate::camera::Camera;
use crate::config::CameraConfig;
use crate::error::Result;
use crate::mat::Mat;

/// OpenCV-style video capture.
/// OpenCV 风格的视频采集。
///
/// Wraps [`Camera`] with automatic format decoding.
/// Each call to [`read()`](Self::read) captures one frame and decodes it to BGR.
///
/// 封装 [`Camera`] 并自动解码格式。
/// 每次调用 [`read()`](Self::read) 采集一帧并解码为 BGR。
pub struct VideoCapture {
    /// The underlying camera device.
    /// 底层摄像头设备。
    camera: Camera,
}

impl VideoCapture {
    /// Open a camera by device index with default settings.
    /// 按设备索引使用默认设置打开摄像头。
    ///
    /// Default: 640x480, 30fps, auto format.
    /// 默认：640x480，30fps，自动格式。
    pub fn open(index: u32) -> Result<Self> {
        let camera = Camera::open(index)?;
        Ok(Self { camera })
    }

    /// Open a camera with custom configuration.
    /// 使用自定义配置打开摄像头。
    pub fn open_with(index: u32, config: CameraConfig) -> Result<Self> {
        let camera = Camera::open_with(index, config)?;
        Ok(Self { camera })
    }

    /// Read the next frame, decoded as BGR, into the provided [`Mat`].
    /// 读取下一帧，解码为 BGR，写入提供的 [`Mat`]。
    ///
    /// The `Mat` is reused across calls — its internal buffer is only
    /// reallocated if the resolution changes. This makes the hot loop
    /// allocation-free after the first frame.
    ///
    /// `Mat` 跨调用复用 —— 仅在分辨率变化时重新分配内部缓冲区。
    /// 这使得在第一帧之后的热循环完全无分配。
    ///
    /// Returns `true` if a frame was successfully captured.
    /// 成功采集到帧时返回 `true`。
    pub fn read(&mut self, mat: &mut Mat) -> Result<bool> {
        self.camera.read_decoded(mat)
    }

    /// Check if the camera is successfully opened.
    /// 检查摄像头是否已成功打开。
    pub fn is_opened(&self) -> bool {
        true // If construction succeeded, it's opened.
             // 如果构造成功，则已打开。
    }

    /// Get the image width.
    /// 获取图像宽度。
    pub fn width(&self) -> u32 {
        self.camera.width()
    }

    /// Get the image height.
    /// 获取图像高度。
    pub fn height(&self) -> u32 {
        self.camera.height()
    }

    /// Access the underlying [`Camera`] for advanced operations.
    /// 访问底层 [`Camera`] 进行高级操作。
    pub fn camera(&self) -> &Camera {
        &self.camera
    }
}
