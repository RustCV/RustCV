use std::fmt;

/// OpenCV-like Matrix structure.
/// Owns its data (Vec<u8>) and supports strided memory layout.
#[derive(Clone)]
pub struct Mat {
    pub data: Vec<u8>,
    pub rows: i32,
    pub cols: i32,
    /// 每一行占用的字节数 (Stride)
    /// 对于 Packed 图像，step = cols * channels
    /// 对于 Padded 图像，step > cols * channels
    pub step: usize,
    pub channels: u8,
}

impl Mat {
    pub fn new(rows: i32, cols: i32, channels: u8) -> Self {
        let step = (cols * channels as i32) as usize;
        let size = (rows as usize) * step;
        Self {
            data: vec![0; size],
            rows,
            cols,
            step,
            channels,
        }
    }

    /// 创建一个空的 Mat (通常用于作为输出 buffer)
    pub fn empty() -> Self {
        Self {
            data: vec![],
            rows: 0,
            cols: 0,
            step: 0,
            channels: 0,
        }
    }

    /// 检查是否为空
    pub fn is_empty(&self) -> bool {
        self.data.is_empty() || self.rows == 0 || self.cols == 0
    }

    /// 获取像素数据的切片 (考虑 Stride)
    pub fn row_bytes(&self, row: i32) -> &[u8] {
        let start = (row as usize) * self.step;
        let end = start + (self.cols as usize * self.channels as usize);
        &self.data[start..end] // 注意：这里我们忽略了行尾的 Padding
    }

    // TODO: 实现 row_bytes_mut, at<T> 等
}

impl fmt::Debug for Mat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Mat")
            .field("rows", &self.rows)
            .field("cols", &self.cols)
            .field("channels", &self.channels)
            .field("step", &self.step)
            .finish()
    }
}
