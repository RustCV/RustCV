use crate::pixel_format::PixelFormat;
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::io::RawFd;

/// 核心帧结构体
/// 使用生命周期 'a 绑定到底层 Ring Buffer，实现零拷贝。
#[derive(Debug)]
pub struct Frame<'a> {
    /// 原始图像数据切片
    pub data: &'a [u8],

    /// 图像宽度 (Pixels)
    pub width: u32,

    /// 图像高度 (Pixels)
    pub height: u32,

    /// 【关键】跨距/步长 (Bytes per line)
    /// GPU 和 SIMD 往往要求 256/512 字节对齐，Stride 可能大于 width * bpp
    pub stride: usize,

    /// 像素格式 (含 Bayer, MJPEG 等)
    pub format: PixelFormat,

    /// 帧索引 (用于底层 Buffer 回收或丢帧统计)
    pub sequence: u64,

    /// 时间戳集合 (工业级同步核心)
    pub timestamp: Timestamp,

    /// 帧级元数据 (曝光、增益、GPIO状态)
    pub metadata: FrameMetadata,

    /// 内部句柄 (用于实现 Backend 特有的互操作，如 export_dmabuf)
    pub backend_handle: &'a dyn BackendBufferHandle,
}

#[derive(Debug, Clone, Copy)]
pub struct Timestamp {
    /// 硬件原始时间戳 (纳秒，单调递增，来源各异)
    pub hw_raw_ns: u64,

    /// 【核心特性】经过 Software PLL 矫正后的系统时间
    /// 对齐到 CLOCK_REALTIME 或 CLOCK_MONOTONIC，消除晶振温漂
    pub system_synced: Duration,
}

#[derive(Debug, Clone, Default)]
pub struct FrameMetadata {
    pub actual_exposure_us: Option<u32>, // 实际曝光时间
    pub actual_gain_db: Option<f32>,     // 实际增益
    pub trigger_fired: bool,             // 是否由硬件触发产生
    pub strobe_active: bool,             // 闪光灯是否点亮
}

/// GPU 互操作特质 (Linux)
/// 允许用户直接获取 DMA-BUF fd 喂给 CUDA/Vulkan
#[cfg(unix)]
pub trait AsDmaBuf {
    /// 获取底层的 DMA-BUF 文件描述符。
    /// 注意：不要关闭它，所有权属于 Backend。
    fn as_dmabuf_fd(&self) -> Option<RawFd>;
}

/// GPU 互操作特质 (Windows)
#[cfg(windows)]
pub trait AsDxResource {
    fn as_resource_handle(&self) -> Option<*mut std::ffi::c_void>;
}

// 内部 Trait，用于 Frame 调用底层的方法
pub trait BackendBufferHandle: std::fmt::Debug + Send + Sync {}

impl BackendBufferHandle for () {}
