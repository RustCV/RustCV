use crate::pixel_format::PixelFormat;

#[derive(Debug, Clone)]
pub struct CameraConfig {
    pub resolution_req: Vec<(u32, u32, Priority)>,
    pub fps_req: Option<(u32, Priority)>,
    pub format_req: Vec<(PixelFormat, Priority)>,
    pub buffer_count: usize,         // Ring Buffer 大小，默认 3
    pub align_stride: Option<usize>, // 强制内存对齐 (如 256字节)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Low = 0,
    Medium = 50,
    High = 100,
    Required = 255, // 必须满足，否则报错
}

impl Default for CameraConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl CameraConfig {
    pub fn new() -> Self {
        Self {
            resolution_req: vec![],
            fps_req: None,
            format_req: vec![],
            buffer_count: 3,
            align_stride: Some(256), // 默认对齐以利于 SIMD
        }
    }

    /// 添加分辨率要求
    pub fn resolution(mut self, w: u32, h: u32, p: Priority) -> Self {
        self.resolution_req.push((w, h, p));
        self
    }

    /// 【补全】添加帧率要求
    pub fn fps(mut self, fps: u32, p: Priority) -> Self {
        self.fps_req = Some((fps, p));
        self
    }

    /// 【补全】添加像素格式要求
    /// 支持传入 PixelFormat 或 FourCC (会自动转换)
    pub fn format<T: Into<PixelFormat>>(mut self, fmt: T, p: Priority) -> Self {
        self.format_req.push((fmt.into(), p));
        self
    }

    /// 【补全】设置缓冲区数量 (默认 3)
    pub fn buffer_count(mut self, count: usize) -> Self {
        self.buffer_count = count;
        self
    }
}
