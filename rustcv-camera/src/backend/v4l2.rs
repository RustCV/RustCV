/// V4L2 backend — direct ioctl implementation for Linux.
/// V4L2 后端 —— Linux 上的直接 ioctl 实现。
///
/// This backend talks to the kernel V4L2 driver via raw ioctls,
/// without any intermediate crate (no `v4l` crate dependency).
/// 此后端通过原始 ioctl 直接与内核 V4L2 驱动通信，
/// 不依赖任何中间 crate（无 `v4l` crate 依赖）。
///
/// Key design decisions from performance testing:
/// 性能测试得出的关键设计决策：
///
/// 1. **Direct DQBUF without poll/select** — saves one syscall per frame.
///    直接 DQBUF 不使用 poll/select —— 每帧节省一次系统调用。
/// 2. **Return only `bytesused` bytes** — MJPEG is ~88KB vs 614KB full buffer.
///    只返回 `bytesused` 字节 —— MJPEG 约 88KB 而非 614KB 完整缓冲区。
/// 3. **Disable `exposure_dynamic_framerate`** after every open — prevents
///    the camera from dropping to 10fps in low light.
///    每次打开后禁用 `exposure_dynamic_framerate` —— 防止摄像头在低光照下降至 10fps。
use std::os::fd::RawFd;

use super::v4l2_sys;
use crate::config::{CameraConfig, ResolvedConfig};
use crate::error::{CameraError, Result};
use crate::pixel_format::PixelFormat;

// ─── MmapBuffer ─────────────────────────────────────────────────────────────

/// A single memory-mapped kernel buffer.
/// 单个内核内存映射缓冲区。
///
/// Wraps a raw pointer returned by `mmap()`.
/// Implements `Drop` to ensure `munmap()` is called — preventing resource leaks.
/// 封装 `mmap()` 返回的裸指针。
/// 实现 `Drop` 确保调用 `munmap()` —— 防止资源泄漏。
struct MmapBuffer {
    /// Pointer to the mapped memory region.
    /// 指向映射内存区域的指针。
    ptr: *mut u8,

    /// Total size of this buffer in bytes.
    /// 此缓冲区的总大小（字节）。
    length: usize,
}

/// # Safety
/// mmap buffers are backed by kernel memory with well-defined access semantics.
/// Only one thread (the capture thread) accesses these buffers at a time.
/// mmap 缓冲区由内核内存支持，具有明确的访问语义。
/// 同一时间只有一个线程（取帧线程）访问这些缓冲区。
unsafe impl Send for MmapBuffer {}

impl Drop for MmapBuffer {
    fn drop(&mut self) {
        v4l2_sys::munmap_buffer(self.ptr, self.length);
    }
}

// ─── V4l2Backend ────────────────────────────────────────────────────────────

/// V4L2 capture backend using direct ioctl calls and mmap buffers.
/// 使用直接 ioctl 调用和 mmap 缓冲区的 V4L2 采集后端。
pub(crate) struct V4l2Backend {
    /// File descriptor for the opened /dev/videoN device.
    /// 已打开的 /dev/videoN 设备的文件描述符。
    fd: RawFd,

    /// Memory-mapped buffers. Index matches V4L2 buffer index.
    /// 内存映射缓冲区。索引与 V4L2 缓冲区索引对应。
    buffers: Vec<MmapBuffer>,

    /// Index of the currently dequeued buffer (held by user via Frame<'a>).
    /// `None` if no buffer is currently held.
    /// 当前被出队的缓冲区索引（由用户通过 Frame<'a> 持有）。
    /// 如果没有缓冲区被持有则为 `None`。
    pending_queue: Option<usize>,

    /// Negotiated image width in pixels.
    /// 协商后的图像宽度（像素）。
    width: u32,

    /// Negotiated image height in pixels.
    /// 协商后的图像高度（像素）。
    height: u32,

    /// Negotiated pixel format.
    /// 协商后的像素格式。
    pixel_format: PixelFormat,

    /// Whether streaming is currently active.
    /// 流是否正在运行。
    streaming: bool,
}

/// Raw frame data returned by [`V4l2Backend::dequeue`].
/// [`V4l2Backend::dequeue`] 返回的原始帧数据。
///
/// The lifetime `'a` is tied to the backend (and its mmap buffers).
/// This ensures the data reference is valid as long as the backend exists.
/// 生命周期 `'a` 绑定到后端（及其 mmap 缓冲区）。
/// 这确保数据引用在后端存在期间始终有效。
pub(crate) struct RawFrame<'a> {
    /// V4L2 buffer index — needed to re-queue the buffer via [`V4l2Backend::queue`].
    /// V4L2 缓冲区索引 —— 通过 [`V4l2Backend::queue`] 归还缓冲区时需要。
    #[allow(dead_code)]
    pub index: usize,

    /// Slice of the mmap buffer containing only the valid frame data (`bytesused` bytes).
    /// mmap 缓冲区的切片，仅包含有效帧数据（`bytesused` 字节）。
    ///
    /// For MJPEG this is typically ~88KB, while the full buffer might be 614KB.
    /// This avoids passing garbage trailing bytes to the JPEG decoder.
    /// 对于 MJPEG，这通常约 88KB，而完整缓冲区可能为 614KB。
    /// 这避免了将尾部垃圾字节传给 JPEG 解码器。
    pub data: &'a [u8],

    pub width: u32,
    pub height: u32,
    pub pixel_format: PixelFormat,

    /// Driver-assigned frame sequence number.
    /// 驱动分配的帧序号。
    ///
    /// If consecutive frames have `sequence` gap > 1, frames were dropped by the driver.
    /// 如果连续帧的 `sequence` 间隔 > 1，说明驱动丢弃了帧。
    pub sequence: u64,

    /// Kernel timestamp in microseconds.
    /// 内核时间戳（微秒）。
    pub timestamp_us: u64,
}

impl V4l2Backend {
    /// Create a new (uninitialized) V4L2 backend.
    /// 创建新的（未初始化的）V4L2 后端。
    pub fn new() -> Self {
        Self {
            fd: -1,
            buffers: Vec::new(),
            pending_queue: None,
            width: 0,
            height: 0,
            pixel_format: PixelFormat::Mjpeg,
            streaming: false,
        }
    }

    /// Open a device, negotiate format, allocate mmap buffers.
    /// 打开设备、协商格式、分配 mmap 缓冲区。
    pub fn open(&mut self, device: &str, config: &CameraConfig) -> Result<ResolvedConfig> {
        // 1. Open device file descriptor.
        // 1. 打开设备文件描述符。
        let fd = v4l2_sys::open_device(device)?;
        self.fd = fd;

        // 2. Check device capabilities.
        // 2. 检查设备能力。
        let caps = v4l2_sys::query_capabilities(fd)?;
        let cap_flags = caps.capabilities;
        if cap_flags & v4l2_sys_mit::V4L2_CAP_VIDEO_CAPTURE == 0 {
            return Err(CameraError::DeviceNotFound(format!(
                "{} does not support video capture",
                device
            )));
        }

        // 3. Negotiate format (resolution + pixel format).
        // 3. 协商格式（分辨率 + 像素格式）。
        let resolved = self.negotiate_format(config)?;

        // 4. Set frame rate.
        // 4. 设置帧率。
        let target_fps = config.fps.unwrap_or(30);
        let _ = v4l2_sys::set_fps(fd, target_fps);

        // 5. Disable exposure_dynamic_framerate.
        // 5. 禁用 exposure_dynamic_framerate。
        //
        // Many laptop cameras default this to ON, causing the firmware to reduce FPS
        // in low light. We learned this the hard way during RustCV debugging:
        // the camera would drop from 30fps to 10fps without any software-side cause.
        // 很多笔记本摄像头默认开启此选项，导致固件在低光照下降低帧率。
        // 这是我们在 RustCV 调试中发现的：摄像头会从 30fps 降到 10fps。
        let _ = v4l2_sys::set_control(fd, v4l2_sys::V4L2_CID_EXPOSURE_AUTO_PRIORITY, 0);

        // 6. Allocate mmap buffers.
        // 6. 分配 mmap 缓冲区。
        self.allocate_buffers(config.buffer_count)?;

        Ok(resolved)
    }

    /// Start video streaming.
    /// 启动视频流。
    pub fn start(&mut self) -> Result<()> {
        if self.streaming {
            return Ok(());
        }

        // Queue all buffers before starting the stream.
        // 启动流前将所有缓冲区入队。
        for i in 0..self.buffers.len() {
            v4l2_sys::queue_buffer(self.fd, i as u32)?;
        }

        v4l2_sys::stream_on(self.fd)?;
        self.streaming = true;
        Ok(())
    }

    /// Stop video streaming.
    /// 停止视频流。
    pub fn stop(&mut self) -> Result<()> {
        if !self.streaming {
            return Ok(());
        }
        v4l2_sys::stream_off(self.fd)?;
        self.streaming = false;
        self.pending_queue = None;
        Ok(())
    }

    /// Dequeue a frame from the driver (blocking).
    /// 从驱动出队一帧（阻塞）。
    ///
    /// This is the **hot path** — called once per frame.
    /// No heap allocations, no poll(), single ioctl syscall.
    /// 这是**热路径** —— 每帧调用一次。
    /// 无堆分配、无 poll()、单次 ioctl 系统调用。
    pub fn dequeue(&mut self) -> Result<RawFrame<'_>> {
        if !self.streaming {
            return Err(CameraError::StreamNotStarted);
        }

        // If the previous frame's buffer hasn't been returned, queue it now.
        // This happens automatically when the user calls next_frame() again.
        // 如果上一帧的缓冲区尚未归还，现在将其入队。
        // 当用户再次调用 next_frame() 时会自动执行此操作。
        if let Some(prev_idx) = self.pending_queue.take() {
            v4l2_sys::queue_buffer(self.fd, prev_idx as u32)?;
        }

        // Blocking DQBUF — the kernel wakes us when a frame is ready.
        // 阻塞 DQBUF —— 帧就绪时内核唤醒我们。
        let v4l2_buf = v4l2_sys::dequeue_buffer(self.fd)?;

        let index = v4l2_buf.index as usize;
        let bytesused = v4l2_buf.bytesused as usize;

        // Record this buffer as pending-queue (will be returned on next dequeue).
        // 记录此缓冲区为待归还状态（下次 dequeue 时归还）。
        self.pending_queue = Some(index);

        // Create a slice of only the valid data portion.
        // For MJPEG: bytesused ≈ 88KB, buffer length ≈ 614KB.
        // 创建仅包含有效数据部分的切片。
        // 对于 MJPEG：bytesused ≈ 88KB，缓冲区总长 ≈ 614KB。
        let data = unsafe {
            let buf = &self.buffers[index];
            std::slice::from_raw_parts(buf.ptr, bytesused)
        };

        Ok(RawFrame {
            index,
            data,
            width: self.width,
            height: self.height,
            pixel_format: self.pixel_format,
            sequence: v4l2_buf.sequence as u64,
            timestamp_us: v4l2_buf.timestamp.tv_sec as u64 * 1_000_000
                + v4l2_buf.timestamp.tv_usec as u64,
        })
    }

    /// Explicitly re-queue a buffer (return it to the driver).
    /// 显式将缓冲区重新入队（归还给驱动）。
    ///
    /// Normally called automatically by [`dequeue`] for the previous buffer.
    /// This is exposed for the Pipeline mode which manages buffers differently.
    /// 通常由 [`dequeue`] 自动为上一个缓冲区调用。
    /// 为 Pipeline 模式暴露此方法，该模式的缓冲区管理方式不同。
    #[allow(dead_code)]
    pub fn queue(&mut self, index: usize) -> Result<()> {
        v4l2_sys::queue_buffer(self.fd, index as u32)?;
        Ok(())
    }

    /// Get the negotiated configuration.
    /// 获取协商后的配置。
    #[allow(dead_code)]
    pub fn width(&self) -> u32 {
        self.width
    }
    #[allow(dead_code)]
    pub fn height(&self) -> u32 {
        self.height
    }
    #[allow(dead_code)]
    pub fn pixel_format(&self) -> PixelFormat {
        self.pixel_format
    }
}

// ─── Private implementation ─────────────────────────────────────────────────

impl V4l2Backend {
    /// Negotiate the best format based on user config and device capabilities.
    /// 根据用户配置和设备能力协商最佳格式。
    ///
    /// Strategy:
    /// 1. If user specified pixel_format → use it (error if unsupported).
    /// 2. If user specified resolution → find formats supporting it.
    /// 3. Auto-select: prefer MJPEG for fps<60, YUYV for fps>=60.
    /// 4. If nothing specified → use device default.
    ///
    /// 策略：
    /// 1. 用户指定了 pixel_format → 直接使用（不支持则报错）。
    /// 2. 用户指定了分辨率 → 查找支持该分辨率的格式。
    /// 3. 自动选择：fps<60 优先 MJPEG，fps>=60 优先 YUYV。
    /// 4. 都未指定 → 使用设备默认值。
    fn negotiate_format(&mut self, config: &CameraConfig) -> Result<ResolvedConfig> {
        let fd = self.fd;

        // Collect all supported formats and their resolutions.
        // 收集所有支持的格式及其分辨率。
        let mut supported = Vec::new();
        let mut fmt_idx = 0;
        while let Ok(desc) = v4l2_sys::enum_formats(fd, fmt_idx) {
            let pf = PixelFormat::from_fourcc(desc.pixelformat);
            // Enumerate frame sizes for this format.
            // 枚举此格式的帧尺寸。
            let mut size_idx = 0;
            while let Ok(size) = v4l2_sys::enum_frame_sizes(fd, desc.pixelformat, size_idx) {
                // V4L2_FRMSIZE_TYPE_DISCRETE = 1
                if size.type_ == 1 {
                    let discrete = unsafe { &size.__bindgen_anon_1.discrete };
                    supported.push((pf, discrete.width, discrete.height));
                }
                size_idx += 1;
            }
            fmt_idx += 1;
        }

        if supported.is_empty() {
            return Err(CameraError::FormatNotSupported);
        }

        // Select the best match.
        // 选择最佳匹配。
        let target_w = config.width.unwrap_or(640);
        let target_h = config.height.unwrap_or(480);
        let target_fps = config.fps.unwrap_or(30);

        let selected = if let Some(pf) = config.pixel_format {
            // User specified format — find matching resolution.
            // 用户指定了格式 —— 查找匹配的分辨率。
            supported
                .iter()
                .filter(|(f, _, _)| *f == pf)
                .min_by_key(|(_, w, h)| {
                    let dw = (*w as i64 - target_w as i64).abs();
                    let dh = (*h as i64 - target_h as i64).abs();
                    dw + dh
                })
                .copied()
                .ok_or(CameraError::FormatNotSupported)?
        } else {
            // Auto-select: score each candidate.
            // 自动选择：对每个候选项评分。
            //
            // Scoring:
            // - Resolution match: lower distance = higher score
            // - Format preference: MJPEG for low fps, YUYV for high fps
            // 评分规则：
            // - 分辨率匹配：距离越小得分越高
            // - 格式偏好：低帧率优先 MJPEG，高帧率优先 YUYV
            *supported
                .iter()
                .min_by_key(|(f, w, h)| {
                    let dw = (*w as i64 - target_w as i64).abs();
                    let dh = (*h as i64 - target_h as i64).abs();
                    let resolution_score = dw + dh;

                    // Format preference penalty.
                    // 格式偏好惩罚。
                    let format_penalty: i64 = if target_fps >= 60 {
                        // High fps: prefer raw formats (lower decode overhead).
                        // 高帧率：优先无压缩格式（解码开销更低）。
                        match f {
                            PixelFormat::Yuyv | PixelFormat::Nv12 => 0,
                            PixelFormat::Mjpeg => 100,
                            _ => 200,
                        }
                    } else {
                        // Normal fps: prefer MJPEG (lower USB bandwidth).
                        // 普通帧率：优先 MJPEG（USB 带宽更低）。
                        match f {
                            PixelFormat::Mjpeg => 0,
                            PixelFormat::Yuyv | PixelFormat::Nv12 => 50,
                            _ => 200,
                        }
                    };

                    resolution_score + format_penalty
                })
                .ok_or(CameraError::FormatNotSupported)?
        };

        let (pf, w, h) = selected;

        // Apply the selected format via VIDIOC_S_FMT.
        // 通过 VIDIOC_S_FMT 应用选定的格式。
        let applied = v4l2_sys::set_format(fd, w, h, pf.to_fourcc())?;
        let actual_pix = unsafe { &applied.fmt.pix };
        self.width = actual_pix.width;
        self.height = actual_pix.height;
        self.pixel_format = PixelFormat::from_fourcc(actual_pix.pixelformat);

        Ok(ResolvedConfig {
            width: self.width,
            height: self.height,
            fps: target_fps,
            pixel_format: self.pixel_format,
            buffer_count: config.buffer_count,
        })
    }

    /// Allocate mmap buffers from the kernel.
    /// 从内核分配 mmap 缓冲区。
    ///
    /// Steps:
    /// 1. VIDIOC_REQBUFS — ask kernel for N buffers
    /// 2. For each buffer: VIDIOC_QUERYBUF → mmap()
    ///
    /// 步骤：
    /// 1. VIDIOC_REQBUFS —— 向内核请求 N 个缓冲区
    /// 2. 对每个缓冲区：VIDIOC_QUERYBUF → mmap()
    fn allocate_buffers(&mut self, count: u32) -> Result<()> {
        let req = v4l2_sys::request_buffers(self.fd, count)?;
        let actual_count = req.count;

        if actual_count < 2 {
            return Err(CameraError::BufferAllocationFailed);
        }

        let mut buffers = Vec::with_capacity(actual_count as usize);

        for i in 0..actual_count {
            let buf = v4l2_sys::query_buffer(self.fd, i)?;
            let length = buf.length as usize;
            let offset = unsafe { buf.m.offset };

            let ptr = v4l2_sys::mmap_buffer(self.fd, length, offset)?;

            buffers.push(MmapBuffer { ptr, length });
        }

        self.buffers = buffers;
        Ok(())
    }
}

impl Drop for V4l2Backend {
    fn drop(&mut self) {
        // Stop streaming if active.
        // 如果流正在运行则停止。
        if self.streaming {
            let _ = self.stop();
        }

        // Release mmap buffers (Drop on each MmapBuffer calls munmap).
        // 释放 mmap 缓冲区（每个 MmapBuffer 的 Drop 会调用 munmap）。
        self.buffers.clear();

        // Tell the kernel to release buffer resources.
        // 通知内核释放缓冲区资源。
        if self.fd >= 0 {
            let _ = v4l2_sys::request_buffers(self.fd, 0);
            v4l2_sys::close_device(self.fd);
        }
    }
}
