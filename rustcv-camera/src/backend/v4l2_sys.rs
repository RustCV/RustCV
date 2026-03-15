/// Low-level V4L2 ioctl wrappers.
/// V4L2 ioctl 底层封装。
///
/// Each function wraps a single V4L2 ioctl call with:
/// - Correct struct initialization (zeroed + required fields)
/// - Error handling (ioctl returns -1 → `io::Error`)
/// - Safe Rust signature (no raw pointers in the public API)
///
/// 每个函数封装一个 V4L2 ioctl 调用：
/// - 正确的结构体初始化（零初始化 + 必填字段）
/// - 错误处理（ioctl 返回 -1 → `io::Error`）
/// - 安全的 Rust 签名（公共 API 不暴露裸指针）
///
/// We use `v4l2-sys-mit` for struct definitions (auto-generated from kernel headers)
/// and `libc` for the ioctl/mmap syscalls.
/// 使用 `v4l2-sys-mit` 获取结构体定义（从内核头文件自动生成），
/// 使用 `libc` 进行 ioctl/mmap 系统调用。
use std::io;
use std::mem;
use std::os::fd::RawFd;
use v4l2_sys_mit::*;

// ─── V4L2 ioctl command numbers ─────────────────────────────────────────────

/// Build V4L2 ioctl command numbers at compile time.
/// 在编译时构造 V4L2 ioctl 命令号。
///
/// Linux ioctl encoding: `direction(2) | size(14) | type(8) | nr(8)`
/// Linux ioctl 编码：`方向(2) | 大小(14) | 类型(8) | 编号(8)`
const fn ioc(dir: u32, typ: u8, nr: u8, size: usize) -> u32 {
    (dir << 30) | ((size as u32 & 0x3FFF) << 16) | ((typ as u32) << 8) | (nr as u32)
}
const fn ior<T>(typ: u8, nr: u8) -> u32 {
    ioc(2, typ, nr, mem::size_of::<T>())
}
const fn iow<T>(typ: u8, nr: u8) -> u32 {
    ioc(1, typ, nr, mem::size_of::<T>())
}
const fn iowr<T>(typ: u8, nr: u8) -> u32 {
    ioc(3, typ, nr, mem::size_of::<T>())
}

const VIDIOC_QUERYCAP: u32 = ior::<v4l2_capability>(b'V', 0);
const VIDIOC_ENUM_FMT: u32 = iowr::<v4l2_fmtdesc>(b'V', 2);
#[allow(dead_code)]
const VIDIOC_G_FMT: u32 = iowr::<v4l2_format>(b'V', 4);
const VIDIOC_S_FMT: u32 = iowr::<v4l2_format>(b'V', 5);
const VIDIOC_REQBUFS: u32 = iowr::<v4l2_requestbuffers>(b'V', 8);
const VIDIOC_QUERYBUF: u32 = iowr::<v4l2_buffer>(b'V', 9);
const VIDIOC_QBUF: u32 = iowr::<v4l2_buffer>(b'V', 15);
const VIDIOC_DQBUF: u32 = iowr::<v4l2_buffer>(b'V', 17);
const VIDIOC_STREAMON: u32 = iow::<u32>(b'V', 18);
const VIDIOC_STREAMOFF: u32 = iow::<u32>(b'V', 19);
const VIDIOC_S_PARM: u32 = iowr::<v4l2_streamparm>(b'V', 22);
const VIDIOC_S_CTRL: u32 = iowr::<v4l2_control>(b'V', 28);
const VIDIOC_ENUM_FRAMESIZES: u32 = iowr::<v4l2_frmsizeenum>(b'V', 74);
#[allow(dead_code)]
const VIDIOC_ENUM_FRAMEINTERVALS: u32 = iowr::<v4l2_frmivalenum>(b'V', 75);

/// V4L2 enum constants (not always exported by v4l2-sys-mit).
/// V4L2 枚举常量（v4l2-sys-mit 不一定导出这些）。
const V4L2_BUF_TYPE_VIDEO_CAPTURE: u32 = 1;
const V4L2_MEMORY_MMAP: u32 = 1;
const V4L2_FIELD_NONE: u32 = 1;

// ─── ioctl helper ───────────────────────────────────────────────────────────

/// Call a V4L2 ioctl and convert errors to `io::Result`.
/// 调用 V4L2 ioctl 并将错误转换为 `io::Result`。
///
/// # Safety
/// The caller must ensure `arg` points to a valid, correctly-typed struct
/// for the given `request` code.
/// 调用者必须确保 `arg` 指向与 `request` 对应的有效且类型正确的结构体。
unsafe fn v4l2_ioctl(fd: RawFd, request: u32, arg: *mut libc::c_void) -> io::Result<()> {
    let ret = libc::ioctl(fd, request as libc::c_ulong, arg);
    if ret == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

// ─── Device capability ──────────────────────────────────────────────────────

/// Query device capabilities (VIDIOC_QUERYCAP).
/// 查询设备能力（VIDIOC_QUERYCAP）。
///
/// Used to check if the device supports video capture (`V4L2_CAP_VIDEO_CAPTURE`)
/// and streaming I/O (`V4L2_CAP_STREAMING`).
/// 用于检查设备是否支持视频采集（`V4L2_CAP_VIDEO_CAPTURE`）
/// 和流式 I/O（`V4L2_CAP_STREAMING`）。
pub fn query_capabilities(fd: RawFd) -> io::Result<v4l2_capability> {
    let mut caps: v4l2_capability = unsafe { std::mem::zeroed() };
    unsafe {
        v4l2_ioctl(
            fd,
            VIDIOC_QUERYCAP,
            &mut caps as *mut _ as *mut libc::c_void,
        )?;
    }
    Ok(caps)
}

// ─── Format enumeration ─────────────────────────────────────────────────────

/// Enumerate supported pixel formats (VIDIOC_ENUM_FMT).
/// 枚举设备支持的像素格式（VIDIOC_ENUM_FMT）。
///
/// Call with `index` starting from 0, incrementing until `EINVAL` is returned
/// (indicating no more formats).
/// `index` 从 0 开始递增调用，直到返回 `EINVAL`（表示枚举完毕）。
pub fn enum_formats(fd: RawFd, index: u32) -> io::Result<v4l2_fmtdesc> {
    let mut desc: v4l2_fmtdesc = unsafe { std::mem::zeroed() };
    desc.index = index;
    desc.type_ = V4L2_BUF_TYPE_VIDEO_CAPTURE;
    unsafe {
        v4l2_ioctl(
            fd,
            VIDIOC_ENUM_FMT,
            &mut desc as *mut _ as *mut libc::c_void,
        )?;
    }
    Ok(desc)
}

/// Enumerate supported frame sizes for a given pixel format (VIDIOC_ENUM_FRAMESIZES).
/// 枚举指定像素格式下支持的帧尺寸（VIDIOC_ENUM_FRAMESIZES）。
pub fn enum_frame_sizes(fd: RawFd, pixel_format: u32, index: u32) -> io::Result<v4l2_frmsizeenum> {
    let mut size: v4l2_frmsizeenum = unsafe { std::mem::zeroed() };
    size.index = index;
    size.pixel_format = pixel_format;
    unsafe {
        v4l2_ioctl(
            fd,
            VIDIOC_ENUM_FRAMESIZES,
            &mut size as *mut _ as *mut libc::c_void,
        )?;
    }
    Ok(size)
}

/// Enumerate supported frame intervals for a given format and size (VIDIOC_ENUM_FRAMEINTERVALS).
/// 枚举指定格式和尺寸下支持的帧间隔（VIDIOC_ENUM_FRAMEINTERVALS）。
#[allow(dead_code)]
pub fn enum_frame_intervals(
    fd: RawFd,
    pixel_format: u32,
    width: u32,
    height: u32,
    index: u32,
) -> io::Result<v4l2_frmivalenum> {
    let mut interval: v4l2_frmivalenum = unsafe { std::mem::zeroed() };
    interval.index = index;
    interval.pixel_format = pixel_format;
    interval.width = width;
    interval.height = height;
    unsafe {
        v4l2_ioctl(
            fd,
            VIDIOC_ENUM_FRAMEINTERVALS,
            &mut interval as *mut _ as *mut libc::c_void,
        )?;
    }
    Ok(interval)
}

// ─── Format get/set ─────────────────────────────────────────────────────────

/// Get the current video format (VIDIOC_G_FMT).
/// 获取当前视频格式（VIDIOC_G_FMT）。
#[allow(dead_code)]
pub fn get_format(fd: RawFd) -> io::Result<v4l2_format> {
    let mut fmt: v4l2_format = unsafe { std::mem::zeroed() };
    fmt.type_ = V4L2_BUF_TYPE_VIDEO_CAPTURE;
    unsafe {
        v4l2_ioctl(fd, VIDIOC_G_FMT, &mut fmt as *mut _ as *mut libc::c_void)?;
    }
    Ok(fmt)
}

/// Set the video format (VIDIOC_S_FMT).
/// 设置视频格式（VIDIOC_S_FMT）。
///
/// The driver may adjust the requested values to the nearest supported ones.
/// The returned struct contains the actual applied values.
/// 驱动可能将请求值调整为最接近的支持值。返回的结构体包含实际应用的值。
pub fn set_format(fd: RawFd, width: u32, height: u32, fourcc: u32) -> io::Result<v4l2_format> {
    let mut fmt: v4l2_format = unsafe { std::mem::zeroed() };
    fmt.type_ = V4L2_BUF_TYPE_VIDEO_CAPTURE;

    // Access the pix member of the anonymous union via raw pointer.
    // 通过裸指针访问匿名联合体的 pix 成员。
    let pix = unsafe { &mut fmt.fmt.pix };
    pix.width = width;
    pix.height = height;
    pix.pixelformat = fourcc;
    pix.field = V4L2_FIELD_NONE;

    unsafe {
        v4l2_ioctl(fd, VIDIOC_S_FMT, &mut fmt as *mut _ as *mut libc::c_void)?;
    }
    Ok(fmt)
}

// ─── Frame rate ─────────────────────────────────────────────────────────────

/// Set the frame rate via VIDIOC_S_PARM.
/// 通过 VIDIOC_S_PARM 设置帧率。
///
/// Frame rate is expressed as a fraction: `timeperframe = numerator / denominator`.
/// For 30fps: numerator=1, denominator=30.
/// 帧率用分数表示：`timeperframe = 分子 / 分母`。
/// 30fps 对应：分子=1，分母=30。
pub fn set_fps(fd: RawFd, fps: u32) -> io::Result<v4l2_streamparm> {
    let mut parm: v4l2_streamparm = unsafe { std::mem::zeroed() };
    parm.type_ = V4L2_BUF_TYPE_VIDEO_CAPTURE;

    let capture = unsafe { &mut parm.parm.capture };
    capture.timeperframe.numerator = 1;
    capture.timeperframe.denominator = fps;

    unsafe {
        v4l2_ioctl(fd, VIDIOC_S_PARM, &mut parm as *mut _ as *mut libc::c_void)?;
    }
    Ok(parm)
}

// ─── Controls ───────────────────────────────────────────────────────────────

/// Set a V4L2 control value (VIDIOC_S_CTRL).
/// 设置 V4L2 控制参数值（VIDIOC_S_CTRL）。
///
/// Common uses:
/// - Disable `exposure_dynamic_framerate` (id=0x009a0903, value=0)
///   to prevent the camera from reducing FPS in low light.
///
/// 常见用途：
/// - 禁用 `exposure_dynamic_framerate`（id=0x009a0903, value=0），
///   防止摄像头在低光照下自动降低帧率。
pub fn set_control(fd: RawFd, id: u32, value: i32) -> io::Result<()> {
    let mut ctrl: v4l2_control = unsafe { std::mem::zeroed() };
    ctrl.id = id;
    ctrl.value = value;
    unsafe {
        v4l2_ioctl(fd, VIDIOC_S_CTRL, &mut ctrl as *mut _ as *mut libc::c_void)?;
    }
    Ok(())
}

// ─── Buffer management ──────────────────────────────────────────────────────

/// Request mmap buffers from the kernel (VIDIOC_REQBUFS).
/// 向内核申请 mmap 缓冲区（VIDIOC_REQBUFS）。
///
/// The kernel may allocate fewer buffers than requested.
/// Check `reqbufs.count` for the actual number.
/// 内核可能分配比请求更少的缓冲区。检查 `reqbufs.count` 获取实际数量。
pub fn request_buffers(fd: RawFd, count: u32) -> io::Result<v4l2_requestbuffers> {
    let mut req: v4l2_requestbuffers = unsafe { std::mem::zeroed() };
    req.count = count;
    req.type_ = V4L2_BUF_TYPE_VIDEO_CAPTURE;
    req.memory = V4L2_MEMORY_MMAP;
    unsafe {
        v4l2_ioctl(fd, VIDIOC_REQBUFS, &mut req as *mut _ as *mut libc::c_void)?;
    }
    Ok(req)
}

/// Query a single buffer's offset and length (VIDIOC_QUERYBUF).
/// 查询单个缓冲区的偏移量和大小（VIDIOC_QUERYBUF）。
///
/// The returned `m.offset` and `length` are needed for `mmap()`.
/// 返回的 `m.offset` 和 `length` 用于 `mmap()` 映射。
pub fn query_buffer(fd: RawFd, index: u32) -> io::Result<v4l2_buffer> {
    let mut buf: v4l2_buffer = unsafe { std::mem::zeroed() };
    buf.index = index;
    buf.type_ = V4L2_BUF_TYPE_VIDEO_CAPTURE;
    buf.memory = V4L2_MEMORY_MMAP;
    unsafe {
        v4l2_ioctl(fd, VIDIOC_QUERYBUF, &mut buf as *mut _ as *mut libc::c_void)?;
    }
    Ok(buf)
}

/// Enqueue a buffer — return it to the driver for filling (VIDIOC_QBUF).
/// 将缓冲区入队 —— 归还给驱动以便其写入新数据（VIDIOC_QBUF）。
pub fn queue_buffer(fd: RawFd, index: u32) -> io::Result<()> {
    let mut buf: v4l2_buffer = unsafe { std::mem::zeroed() };
    buf.index = index;
    buf.type_ = V4L2_BUF_TYPE_VIDEO_CAPTURE;
    buf.memory = V4L2_MEMORY_MMAP;
    unsafe {
        v4l2_ioctl(fd, VIDIOC_QBUF, &mut buf as *mut _ as *mut libc::c_void)?;
    }
    Ok(())
}

/// Dequeue a buffer — retrieve a filled buffer from the driver (VIDIOC_DQBUF).
/// 将缓冲区出队 —— 从驱动取回已写满数据的缓冲区（VIDIOC_DQBUF）。
///
/// This call **blocks** until a frame is ready. No `poll()`/`select()` is used,
/// which saves one syscall per frame compared to the `v4l` crate's approach.
/// 此调用会**阻塞**直到帧就绪。不使用 `poll()`/`select()`，
/// 相比 `v4l` crate 的做法每帧节省一次系统调用。
///
/// The returned `v4l2_buffer` contains:
/// - `index`: which buffer was filled
/// - `bytesused`: actual data size (important for MJPEG — much smaller than buffer size)
/// - `sequence`: driver-assigned frame sequence number (for drop detection)
/// - `timestamp`: kernel timestamp
///
/// 返回的 `v4l2_buffer` 包含：
/// - `index`：被填充的缓冲区序号
/// - `bytesused`：实际数据大小（对 MJPEG 很重要 —— 远小于缓冲区总大小）
/// - `sequence`：驱动分配的帧序号（用于丢帧检测）
/// - `timestamp`：内核时间戳
pub fn dequeue_buffer(fd: RawFd) -> io::Result<v4l2_buffer> {
    let mut buf: v4l2_buffer = unsafe { std::mem::zeroed() };
    buf.type_ = V4L2_BUF_TYPE_VIDEO_CAPTURE;
    buf.memory = V4L2_MEMORY_MMAP;
    unsafe {
        v4l2_ioctl(fd, VIDIOC_DQBUF, &mut buf as *mut _ as *mut libc::c_void)?;
    }
    Ok(buf)
}

// ─── Stream control ─────────────────────────────────────────────────────────

/// Start video streaming (VIDIOC_STREAMON).
/// 启动视频流（VIDIOC_STREAMON）。
///
/// All buffers must be queued before calling this.
/// 调用前所有缓冲区必须已入队。
pub fn stream_on(fd: RawFd) -> io::Result<()> {
    let mut buf_type: u32 = V4L2_BUF_TYPE_VIDEO_CAPTURE;
    unsafe {
        v4l2_ioctl(
            fd,
            VIDIOC_STREAMON,
            &mut buf_type as *mut _ as *mut libc::c_void,
        )?;
    }
    Ok(())
}

/// Stop video streaming (VIDIOC_STREAMOFF).
/// 停止视频流（VIDIOC_STREAMOFF）。
///
/// All buffers are automatically dequeued by the driver after this call.
/// 此调用后驱动会自动将所有缓冲区出队。
pub fn stream_off(fd: RawFd) -> io::Result<()> {
    let mut buf_type: u32 = V4L2_BUF_TYPE_VIDEO_CAPTURE;
    unsafe {
        v4l2_ioctl(
            fd,
            VIDIOC_STREAMOFF,
            &mut buf_type as *mut _ as *mut libc::c_void,
        )?;
    }
    Ok(())
}

// ─── Device open/close ──────────────────────────────────────────────────────

/// Open a V4L2 device node (e.g., "/dev/video0").
/// 打开 V4L2 设备节点（如 "/dev/video0"）。
///
/// Opens with `O_RDWR` (read/write) without `O_NONBLOCK`.
/// Without `O_NONBLOCK`, `DQBUF` will block until a frame is ready,
/// which is the desired behavior for maximum throughput.
///
/// 以 `O_RDWR`（读写）模式打开，不加 `O_NONBLOCK`。
/// 不加 `O_NONBLOCK` 时，`DQBUF` 会阻塞直到帧就绪，
/// 这是取得最大吞吐量的期望行为。
pub fn open_device(path: &str) -> io::Result<RawFd> {
    let c_path =
        std::ffi::CString::new(path).map_err(|_| io::Error::other("invalid device path"))?;
    let fd = unsafe { libc::open(c_path.as_ptr(), libc::O_RDWR) };
    if fd == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(fd)
    }
}

/// Close a V4L2 device file descriptor.
/// 关闭 V4L2 设备文件描述符。
pub fn close_device(fd: RawFd) {
    unsafe {
        libc::close(fd);
    }
}

// ─── mmap ───────────────────────────────────────────────────────────────────

/// Memory-map a V4L2 buffer into user space.
/// 将 V4L2 缓冲区内存映射到用户空间。
///
/// Uses `MAP_SHARED` so user space and kernel see the same physical memory (zero-copy).
/// Uses `PROT_READ | PROT_WRITE` — read for frame data access, write for kernel DMA.
///
/// 使用 `MAP_SHARED` 使用户空间和内核看到同一块物理内存（零拷贝）。
/// 使用 `PROT_READ | PROT_WRITE` —— 读用于帧数据访问，写用于内核 DMA。
pub fn mmap_buffer(fd: RawFd, length: usize, offset: u32) -> io::Result<*mut u8> {
    let ptr = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            length,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            fd,
            offset as libc::off_t,
        )
    };
    if ptr == libc::MAP_FAILED {
        Err(io::Error::last_os_error())
    } else {
        Ok(ptr as *mut u8)
    }
}

/// Unmap a previously mapped buffer.
/// 解除先前映射的缓冲区。
pub fn munmap_buffer(ptr: *mut u8, length: usize) {
    unsafe {
        libc::munmap(ptr as *mut libc::c_void, length);
    }
}

// ─── V4L2 constants not in v4l2-sys-mit ─────────────────────────────────────

/// V4L2_CID_EXPOSURE_AUTO_PRIORITY — controls dynamic framerate.
/// V4L2_CID_EXPOSURE_AUTO_PRIORITY —— 控制动态帧率。
///
/// When set to 1 (default on many laptops), the camera firmware can
/// reduce FPS in low-light conditions for longer exposure.
/// We disable this (set to 0) to maintain consistent frame rate.
///
/// 设为 1 时（很多笔记本的默认值），摄像头固件会在低光照下
/// 降低帧率以延长曝光时间。
/// 我们将其设为 0 以保持稳定的帧率。
pub const V4L2_CID_EXPOSURE_AUTO_PRIORITY: u32 = 0x009a0903;
