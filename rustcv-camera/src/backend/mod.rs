/// Backend selection via compile-time `cfg`.
/// 通过编译时 `cfg` 选择后端。
///
/// Each platform has its own capture backend:
/// - Linux:   V4L2 (direct ioctl, zero-copy mmap)
/// - macOS:   AVFoundation (ObjC bridge, BGRA32 via callback)
/// - Windows: Media Foundation (stub — not yet implemented)
///
/// 每个平台有各自的采集后端：
/// - Linux：  V4L2（直接 ioctl，零拷贝 mmap）
/// - macOS：  AVFoundation（ObjC 桥接，回调 BGRA32）
/// - Windows：Media Foundation（存根 —— 尚未实现）
use crate::pixel_format::PixelFormat;

// ─── Shared types ────────────────────────────────────────────────────────────

/// Raw frame data returned by a platform backend's `dequeue()`.
/// 平台后端 `dequeue()` 返回的原始帧数据。
///
/// The lifetime `'a` is tied to the backend that produced it.
/// For V4L2 this borrows directly from the mmap buffer; for AVFoundation
/// it borrows from `AvfBackend::frame_buf`.
///
/// 生命周期 `'a` 绑定到产生它的后端。
/// V4L2 中直接借用 mmap 缓冲区；AVFoundation 中借用 `AvfBackend::frame_buf`。
pub(crate) struct RawFrame<'a> {
    /// Buffer index — used by V4L2 to re-queue the mmap buffer.
    /// Always 0 on AVFoundation (buffers managed internally).
    ///
    /// 缓冲区索引 —— V4L2 用于重新入队 mmap 缓冲区。
    /// AVFoundation 上始终为 0（缓冲区由内部管理）。
    #[allow(dead_code)]
    pub index: usize,

    /// Pixel data slice. Format is indicated by `pixel_format`.
    /// 像素数据切片。格式由 `pixel_format` 指示。
    pub data: &'a [u8],

    pub width: u32,
    pub height: u32,
    pub pixel_format: PixelFormat,

    /// Monotonically increasing frame counter.
    /// V4L2: kernel sequence number (gaps indicate dropped frames).
    /// AVFoundation: simple counter incremented by the delegate.
    ///
    /// 单调递增的帧计数器。
    /// V4L2：内核序号（间隔表示丢帧）。
    /// AVFoundation：delegate 递增的简单计数器。
    pub sequence: u64,

    /// Capture timestamp in microseconds.
    /// 采集时间戳（微秒）。
    pub timestamp_us: u64,
}

// ─── Platform backends ───────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
pub(crate) mod linux;

#[cfg(target_os = "macos")]
pub(crate) mod macos;

#[cfg(target_os = "windows")]
pub(crate) mod windows;

// ─── PlatformBackend type alias ──────────────────────────────────────────────

#[cfg(target_os = "linux")]
pub(crate) use linux::V4l2Backend as PlatformBackend;

#[cfg(target_os = "macos")]
pub(crate) use macos::AvfBackend as PlatformBackend;

#[cfg(target_os = "windows")]
pub(crate) use windows::MsmfBackend as PlatformBackend;
