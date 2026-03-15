/// Error types for the rustcv-camera crate.
/// rustcv-camera 的错误类型定义。
///
/// Uses `thiserror` for ergonomic error definitions.
/// Library crates should use `thiserror` (not `anyhow`) so callers can match on specific errors.
/// 使用 `thiserror` 定义错误类型。
/// 库应使用 `thiserror`（而非 `anyhow`），以便调用者可以 match 具体的错误变体。
use std::io;

/// All possible errors returned by this crate.
/// 本 crate 可能返回的所有错误类型。
#[derive(Debug, thiserror::Error)]
pub enum CameraError {
    /// The specified camera device was not found.
    /// 指定的摄像头设备未找到。
    #[error("device not found: {0}")]
    DeviceNotFound(String),

    /// The device is already in use by another process.
    /// 设备已被其他进程占用。
    #[error("device busy")]
    DeviceBusy,

    /// The requested pixel format is not supported by the device.
    /// 设备不支持请求的像素格式。
    #[error("format not supported")]
    FormatNotSupported,

    /// The requested resolution is not supported by the device.
    /// 设备不支持请求的分辨率。
    #[error("resolution not supported: {0}x{1}")]
    ResolutionNotSupported(u32, u32),

    /// Attempted to capture frames without starting the stream first.
    /// 在未启动流的情况下尝试取帧。
    #[error("stream not started")]
    StreamNotStarted,

    /// Failed to allocate kernel mmap buffers.
    /// 内核 mmap 缓冲区分配失败。
    #[error("buffer allocation failed")]
    BufferAllocationFailed,

    /// Error during pixel format decoding (MJPEG, YUYV, etc.).
    /// 像素格式解码（MJPEG、YUYV 等）时出错。
    #[error("decode error: {0}")]
    DecodeError(String),

    /// Underlying OS I/O error.
    /// 底层操作系统 I/O 错误。
    ///
    /// The `#[from]` attribute auto-implements `From<io::Error>`,
    /// so the `?` operator converts `io::Error` automatically.
    /// `#[from]` 自动实现 `From<io::Error>`，使 `?` 运算符能自动转换。
    #[error("io error: {0}")]
    Io(#[from] io::Error),
}

/// A type alias for `Result<T, CameraError>`.
/// `Result<T, CameraError>` 的类型别名。
///
/// Every fallible function in this crate returns this type,
/// avoiding repetitive `Result<T, CameraError>` signatures.
/// 本 crate 中所有可失败函数都返回此类型，避免重复书写完整签名。
pub type Result<T> = std::result::Result<T, CameraError>;
