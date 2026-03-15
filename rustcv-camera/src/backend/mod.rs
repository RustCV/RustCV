/// Backend selection via compile-time `cfg`.
/// 通过编译时 `cfg` 选择后端。
///
/// Each platform has its own capture backend:
/// - Linux: V4L2 (direct ioctl, this crate's focus)
/// - macOS: AVFoundation (future)
/// - Windows: Media Foundation (future)
///
/// 每个平台有各自的采集后端：
/// - Linux：V4L2（直接 ioctl，本 crate 的重点）
/// - macOS：AVFoundation（未来实现）
/// - Windows：Media Foundation（未来实现）
#[cfg(target_os = "linux")]
pub(crate) mod v4l2_sys;

#[cfg(target_os = "linux")]
pub(crate) mod v4l2;

#[cfg(target_os = "linux")]
pub(crate) use v4l2::V4l2Backend as PlatformBackend;
