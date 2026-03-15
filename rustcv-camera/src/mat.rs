/// BGR image container, compatible with OpenCV Mat semantics.
/// BGR 图像容器，兼容 OpenCV Mat 语义。
///
/// Stores decoded pixel data in BGR channel order (Blue, Green, Red),
/// which is OpenCV's default. The data is contiguous in memory,
/// row by row (no padding between rows in the default case).
///
/// 以 BGR 通道顺序（蓝、绿、红）存储解码后的像素数据，
/// 这是 OpenCV 的默认格式。数据在内存中连续存储，
/// 逐行排列（默认情况下行间无填充）。
///
/// The internal `Vec<u8>` is reused across frames to avoid per-frame allocation.
/// Call [`VideoCapture::read()`] repeatedly with the same `Mat` for best performance.
/// 内部的 `Vec<u8>` 跨帧复用以避免每帧分配。
/// 使用同一个 `Mat` 重复调用 [`VideoCapture::read()`] 可获得最佳性能。
///
/// A decoded BGR image.
/// 已解码的 BGR 图像。
#[derive(Debug, Clone)]
pub struct Mat {
    /// Raw BGR pixel data. Layout: `[B, G, R, B, G, R, ...]`.
    /// 原始 BGR 像素数据。布局：`[B, G, R, B, G, R, ...]`。
    pub(crate) data: Vec<u8>,

    /// Number of rows (image height).
    /// 行数（图像高度）。
    pub(crate) rows: u32,

    /// Number of columns (image width).
    /// 列数（图像宽度）。
    pub(crate) cols: u32,

    /// Number of channels (always 3 for BGR).
    /// 通道数（BGR 固定为 3）。
    pub(crate) channels: u32,

    /// Row step in bytes (cols * channels).
    /// 行步长（字节）（cols * channels）。
    pub(crate) step: usize,
}

impl Mat {
    /// Create an empty Mat with no allocated data.
    /// 创建一个无数据分配的空 Mat。
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            rows: 0,
            cols: 0,
            channels: 3,
            step: 0,
        }
    }

    /// Ensure the internal buffer is large enough for the given dimensions.
    /// 确保内部缓冲区对于给定尺寸足够大。
    ///
    /// If the size matches the current allocation, no reallocation occurs.
    /// This is key for zero-allocation frame decoding: call `read()` in a loop
    /// with the same Mat, and memory is allocated only once (on the first frame).
    ///
    /// 如果大小与当前分配匹配，则不会重新分配。
    /// 这是零分配帧解码的关键：在循环中使用同一个 Mat 调用 `read()`，
    /// 内存仅分配一次（在第一帧时）。
    pub(crate) fn ensure_size(&mut self, rows: u32, cols: u32, channels: u32) {
        let needed = (rows as usize) * (cols as usize) * (channels as usize);
        if self.data.len() != needed {
            self.data.resize(needed, 0);
        }
        self.rows = rows;
        self.cols = cols;
        self.channels = channels;
        self.step = (cols as usize) * (channels as usize);
    }

    /// Returns `true` if this Mat contains no data.
    /// 如果此 Mat 不包含数据则返回 `true`。
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Access the raw pixel data as a byte slice.
    /// 以字节切片形式访问原始像素数据。
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Access the raw pixel data as a mutable byte slice.
    /// 以可变字节切片形式访问原始像素数据。
    pub fn data_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }

    /// Image height (number of rows).
    /// 图像高度（行数）。
    pub fn rows(&self) -> u32 {
        self.rows
    }

    /// Image width (number of columns).
    /// 图像宽度（列数）。
    pub fn cols(&self) -> u32 {
        self.cols
    }

    /// Number of channels (always 3 for BGR).
    /// 通道数（BGR 固定为 3）。
    pub fn channels(&self) -> u32 {
        self.channels
    }

    /// Row step in bytes.
    /// 行步长（字节）。
    pub fn step(&self) -> usize {
        self.step
    }

    /// Total number of pixels (rows * cols).
    /// 总像素数（rows * cols）。
    pub fn total(&self) -> usize {
        (self.rows as usize) * (self.cols as usize)
    }
}

impl Default for Mat {
    fn default() -> Self {
        Self::new()
    }
}
