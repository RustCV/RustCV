/// Zero-copy frame types.
/// 零拷贝帧类型。
///
/// [`Frame`] borrows directly from the kernel's mmap buffer — no copies.
/// Rust's lifetime system guarantees at compile time that the frame data
/// cannot outlive the camera's buffer pool.
///
/// [`Frame`] 直接借用内核的 mmap 缓冲区 —— 无拷贝。
/// Rust 的生命周期系统在编译时保证帧数据不会比摄像头的缓冲池活得更久。
///
/// Compare with other approaches:
/// 与其他方案的对比：
/// - C/C++ (OpenCV): reference counting at runtime (overhead + possible leaks)
/// - nokhwa: clones buffer data into Vec (copy overhead)
/// - rustcv-camera: compile-time lifetime check (zero overhead)
///
/// - C/C++（OpenCV）：运行时引用计数（有开销 + 可能泄漏）
/// - nokhwa：将缓冲区数据 clone 到 Vec（拷贝开销）
/// - rustcv-camera：编译时生命周期检查（零开销）
use std::marker::PhantomData;

use crate::error::Result;
use crate::mat::Mat;
use crate::pixel_format::PixelFormat;

/// A zero-copy frame borrowing data from the camera's mmap buffer.
/// 零拷贝帧，借用摄像头 mmap 缓冲区中的数据。
///
/// The lifetime `'a` is tied to the [`Camera`](crate::Camera) that produced it.
/// While a `Frame` exists, the camera cannot produce the next frame
/// (enforced by `&mut self` borrow on `next_frame()`).
///
/// 生命周期 `'a` 绑定到产生它的 [`Camera`](crate::Camera)。
/// 当 `Frame` 存在时，摄像头无法产生下一帧
/// （由 `next_frame()` 的 `&mut self` 借用强制保证）。
///
/// # Examples
///
/// ```ignore
/// let mut cam = Camera::open(0)?;
///
/// // OK: frame is dropped before next call
/// let frame = cam.next_frame()?;
/// let data = frame.data();
/// drop(frame);
/// let frame2 = cam.next_frame()?;
///
/// // COMPILE ERROR: cannot borrow cam mutably twice
/// // let frame = cam.next_frame()?;
/// // let frame2 = cam.next_frame()?; // error!
/// ```
pub struct Frame<'a> {
    /// Raw pixel data from the mmap buffer. Only contains `bytesused` bytes.
    /// 来自 mmap 缓冲区的原始像素数据。仅包含 `bytesused` 字节。
    data: &'a [u8],

    /// Image width in pixels.
    /// 图像宽度（像素）。
    width: u32,

    /// Image height in pixels.
    /// 图像高度（像素）。
    height: u32,

    /// Pixel format of the raw data (e.g., MJPEG, YUYV).
    /// 原始数据的像素格式（如 MJPEG、YUYV）。
    pixel_format: PixelFormat,

    /// Driver-assigned frame sequence number.
    /// 驱动分配的帧序号。
    ///
    /// Gaps in sequence numbers indicate dropped frames.
    /// 序号中的间隔表示发生了丢帧。
    sequence: u64,

    /// Kernel capture timestamp in microseconds.
    /// 内核采集时间戳（微秒）。
    timestamp_us: u64,

    /// Marker to tie the lifetime to the camera/backend.
    /// 用于将生命周期绑定到摄像头/后端的标记。
    _marker: PhantomData<&'a ()>,
}

impl<'a> Frame<'a> {
    /// Create a new Frame (crate-internal constructor).
    /// 创建新的 Frame（crate 内部构造函数）。
    pub(crate) fn new(
        data: &'a [u8],
        width: u32,
        height: u32,
        pixel_format: PixelFormat,
        sequence: u64,
        timestamp_us: u64,
    ) -> Self {
        Self {
            data,
            width,
            height,
            pixel_format,
            sequence,
            timestamp_us,
            _marker: PhantomData,
        }
    }

    /// Access the raw pixel data (zero-copy).
    /// 访问原始像素数据（零拷贝）。
    ///
    /// The format of this data depends on [`pixel_format()`](Self::pixel_format):
    /// - `Mjpeg`: JPEG-compressed bytes (needs decoding)
    /// - `Yuyv`: YUYV 4:2:2 packed bytes (needs conversion)
    /// - `Bgr24`: Ready-to-use BGR pixels
    ///
    /// 数据格式取决于 [`pixel_format()`](Self::pixel_format)：
    /// - `Mjpeg`：JPEG 压缩字节（需要解码）
    /// - `Yuyv`：YUYV 4:2:2 打包字节（需要转换）
    /// - `Bgr24`：可直接使用的 BGR 像素
    pub fn data(&self) -> &[u8] {
        self.data
    }

    /// Image width in pixels.
    /// 图像宽度（像素）。
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Image height in pixels.
    /// 图像高度（像素）。
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Pixel format of the raw data.
    /// 原始数据的像素格式。
    pub fn pixel_format(&self) -> PixelFormat {
        self.pixel_format
    }

    /// Driver-assigned frame sequence number.
    /// 驱动分配的帧序号。
    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Kernel capture timestamp in microseconds.
    /// 内核采集时间戳（微秒）。
    pub fn timestamp_us(&self) -> u64 {
        self.timestamp_us
    }

    /// Copy the raw data into an [`OwnedFrame`] that can outlive the camera borrow.
    /// 将原始数据拷贝到 [`OwnedFrame`]，使其可以超出摄像头的借用生命周期。
    ///
    /// Use this when you need to:
    /// - Send frame data to another thread
    /// - Keep frame data after calling `next_frame()` again
    /// - Store frames in a collection
    ///
    /// 在以下场景使用：
    /// - 将帧数据发送到另一个线程
    /// - 在再次调用 `next_frame()` 后仍保留帧数据
    /// - 将帧存储到集合中
    pub fn to_owned(&self) -> OwnedFrame {
        OwnedFrame {
            data: self.data.to_vec(),
            width: self.width,
            height: self.height,
            pixel_format: self.pixel_format,
            sequence: self.sequence,
            timestamp_us: self.timestamp_us,
        }
    }

    /// Decode this frame into a BGR [`Mat`].
    /// 将此帧解码为 BGR [`Mat`]。
    ///
    /// This is a convenience method that allocates a new Mat.
    /// For zero-allocation decoding, use [`Camera::read_decoded()`]
    /// or [`VideoCapture::read()`] which reuse an existing Mat.
    ///
    /// 这是一个便捷方法，会分配新的 Mat。
    /// 如需零分配解码，请使用可复用现有 Mat 的
    /// [`Camera::read_decoded()`] 或 [`VideoCapture::read()`]。
    pub fn decode_bgr(&self) -> Result<Mat> {
        let mut mat = Mat::new();
        crate::decode::decode_frame(self, &mut mat, None)?;
        Ok(mat)
    }
}

/// An owned frame with heap-allocated data.
/// 拥有堆分配数据的帧。
///
/// Unlike [`Frame`], this has no lifetime constraints — it can be freely
/// stored, sent across threads, or kept indefinitely.
///
/// 与 [`Frame`] 不同，此类型没有生命周期约束 —— 可以自由
/// 存储、跨线程发送或无限期持有。
///
/// Created via [`Frame::to_owned()`].
/// 通过 [`Frame::to_owned()`] 创建。
#[derive(Debug, Clone)]
pub struct OwnedFrame {
    data: Vec<u8>,
    width: u32,
    height: u32,
    pixel_format: PixelFormat,
    sequence: u64,
    timestamp_us: u64,
}

impl OwnedFrame {
    pub fn data(&self) -> &[u8] {
        &self.data
    }
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn pixel_format(&self) -> PixelFormat {
        self.pixel_format
    }
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn timestamp_us(&self) -> u64 {
        self.timestamp_us
    }
}
