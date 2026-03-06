// examples/camera_view.rs
//
// macOS AVFoundation 摄像头可视化测试程序
//
// 使用方式：
//   cargo run --example camera_view --target aarch64-apple-darwin
//
// 按 ESC 退出，关闭窗口后摄像头指示灯自动熄灭。
//
// ======================================================================
//  架构说明
// ======================================================================
// 本例严格复刻 V4L2 demo 的 API 使用范式：
//   Driver::list_devices() → Driver::open(id, config)
//   stream.start() → loop { stream.next_frame() } → stream.stop()
//
// OSD 文字渲染采用内嵌 5×7 点阵字库（无需外部 image/imageproc 依赖），
// 直接在 minifb 的 u32 缓冲区上绘制 FPS 和分辨率信息。
// ======================================================================

// ─── macOS 专属主函数 ───────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
use anyhow::{Context, Result};
#[cfg(target_os = "macos")]
use minifb::{Key, Window, WindowOptions};
#[cfg(target_os = "macos")]
use rustcv_backend_avf::AvfDriver;
#[cfg(target_os = "macos")]
use rustcv_core::builder::{CameraConfig, Priority};
#[cfg(target_os = "macos")]
use rustcv_core::pixel_format::FourCC;
#[cfg(target_os = "macos")]
use rustcv_core::traits::Driver;
#[cfg(target_os = "macos")]
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
#[tokio::main]
async fn main() -> Result<()> {
    println!("=== RustCV AVFoundation Backend Demo ===");

    // ── 步骤 1：实例化驱动 ──────────────────────────────────────────────
    let driver = AvfDriver::new();

    // ── 步骤 2：枚举并打印设备列表 ─────────────────────────────────────
    let devices = driver.list_devices()?;
    if devices.is_empty() {
        anyhow::bail!("No cameras found! Please connect a camera.");
    }

    println!("Found {} device(s):", devices.len());
    for (i, dev) in devices.iter().enumerate() {
        println!("  [{}] {} (id: {})", i, dev.name, dev.id,);
    }

    // 默认使用第一个设备
    let target = &devices[0];
    println!("\nOpening device: {}", target.name);

    let width: usize = 640;
    let height: usize = 480;

    // ── 步骤 3：构建采集配置 ────────────────────────────────────────────
    // macOS AVFoundation 返回 32BGRA 格式，直接对接 minifb 最高效。
    // Priority::Required 强制要求分辨率，fps 和 format 作为偏好请求。
    let config = CameraConfig::new()
        .resolution(width as u32, height as u32, Priority::Required)
        .fps(30, Priority::High)
        .format(FourCC::new(b'B', b'G', b'R', b'A'), Priority::High);

    // ── 步骤 4：打开设备，获取 Stream 和 Controls ─────────────────────
    let (mut stream, _controls) = driver
        .open(&target.id, config)
        .context("Failed to open camera device")?;

    // ── 步骤 5：启动流 ─────────────────────────────────────────────────
    stream.start().await.context("Failed to start stream")?;
    println!("Stream started! Press ESC to exit.");

    // ── 创建 minifb 窗口 ───────────────────────────────────────────────
    let mut window = Window::new(
        "RustCV AVF — Camera Preview (ESC to quit)",
        width,
        height,
        WindowOptions::default(),
    )
    .context("Failed to create window")?;

    // minifb 0.24 上允许设置最大帧率（避免 CPU 空转）
    window.limit_update_rate(Some(std::time::Duration::from_micros(33_333))); // ~30fps

    // ── FPS 计量 ───────────────────────────────────────────────────────
    let mut last_fps_time = Instant::now();
    let mut frame_count: u32 = 0;
    let mut display_fps: f64 = 0.0;

    // minifb 的像素缓冲区格式：0x00RRGGBB（每像素 u32）
    // 初始大小与窗口一致；首帧后按实际摄像头分辨率自动重新分配。
    let mut rgb_buffer: Vec<u32> = vec![0u32; width * height];
    // 当前缓冲区/窗口实际使用的宽高（AVFoundation 可能因编解码对齐而返回不同尺寸）
    let mut win_w = width;
    let mut win_h = height;

    // ── 主循环 ─────────────────────────────────────────────────────────
    while window.is_open() && !window.is_key_down(Key::Escape) {
        // ── 步骤 6：获取下一帧 ─────────────────────────────────────────
        let frame = stream.next_frame().await.context("next_frame failed")?;

        // ── 步骤 7：处理分辨率变化 + BGRA → RGB32 转换 ─────────────────
        let actual_width = frame.width as usize;
        let actual_height = frame.height as usize;
        let stride = frame.stride; // bytes_per_row，可能 > width * 4

        // 如果摄像头实际返回的分辨率与窗口不符（AVFoundation 有时因编解码对齐
        // 而让 height 多出几行），则重新分配缓冲区并调整窗口尺寸。
        if actual_width != win_w || actual_height != win_h {
            println!(
                "Frame size changed: {}x{} → {}x{}",
                win_w, win_h, actual_width, actual_height
            );
            win_w = actual_width;
            win_h = actual_height;
            rgb_buffer.resize(win_w * win_h, 0);
            // minifb 不支持动态改 resize，需重建窗口
            window = Window::new(
                "RustCV AVF — Camera Preview (ESC to quit)",
                win_w,
                win_h,
                WindowOptions::default(),
            )
            .context("Failed to recreate window")?;
            window.limit_update_rate(Some(std::time::Duration::from_micros(33_333)));
        }

        // 安全拷贝：即使 height 不整除，也绝不超出 rgb_buffer 边界
        let safe_h = actual_height.min(rgb_buffer.len() / actual_width.max(1));
        bgra_to_rgb32(frame.data, &mut rgb_buffer, actual_width, safe_h, stride);

        // ── 步骤 8：FPS 统计（每秒刷新一次显示值）────────────────────
        frame_count += 1;
        let elapsed = last_fps_time.elapsed();
        if elapsed >= Duration::from_secs(1) {
            display_fps = frame_count as f64 / elapsed.as_secs_f64();
            last_fps_time = Instant::now();
            frame_count = 0;
        }

        // ── 步骤 9：OSD 叠加（左上角绘制 FPS 和分辨率）─────────────
        let osd_line1 = format!("FPS: {:.1}", display_fps);
        let osd_line2 = format!("{}x{}", actual_width, actual_height);
        draw_text_osd(&mut rgb_buffer, win_w, &osd_line1, 10, 8, 0x00FFFF00);
        draw_text_osd(&mut rgb_buffer, win_w, &osd_line2, 10, 20, 0x00FFFF00);

        // ── 步骤 10：刷新窗口 ──────────────────────────────────────────
        window
            .update_with_buffer(&rgb_buffer, win_w, win_h)
            .context("Window update failed")?;
    }

    // ── 步骤 11：优雅退出 ──────────────────────────────────────────────
    // 必须调用 stop()，确保 macOS 摄像头绿色指示灯熄灭
    stream.stop().await.context("Failed to stop stream")?;
    println!("Stream stopped. Camera indicator light off.");

    Ok(())
}

// ─── 非 macOS 平台：空 main ────────────────────────────────────────────────

/// 在非 macOS 系统跳过运行，打印说明信息。
#[cfg(not(target_os = "macos"))]
fn main() {
    println!("This example requires macOS (AVFoundation). Skipping.");
}

// ======================================================================
//  像素格式转换
// ======================================================================

/// 将 BGRA 字节流（AVFoundation 默认输出）转换为 minifb 所需的 RGB32 缓冲区。
///
/// # 格式说明
/// - 输入：每像素 4 字节，内存排布为 [B, G, R, A, B, G, R, A, ...]
/// - 输出：每像素 1 个 u32，格式为 0x00RRGGBB
///
/// # 步长（Stride）处理
/// CVPixelBuffer 的 bytes_per_row 可能因 GPU 对齐而大于 width × 4，
/// 因此必须按行处理，而非直接线性遍历整个字节数组。
///
/// # 位操作说明
/// - R 分量在 BGRA 中位于索引 +2，需移到 0x00RRGGBB 的位 16..23：`(r as u32) << 16`
/// - G 分量位于索引 +1，移到位 8..15：`(g as u32) << 8`
/// - B 分量位于索引 +0，直接放在位 0..7：`b as u32`
#[cfg(target_os = "macos")]
fn bgra_to_rgb32(
    src: &[u8],      // AVFoundation 返回的 BGRA 原始像素数据
    dst: &mut [u32], // minifb 的 0x00RRGGBB 缓冲区
    width: usize,    // 图像有效宽度（像素数）
    height: usize,   // 图像有效高度（像素数）
    stride: usize,   // 每行实际字节数（bytes_per_row，含填充）
) {
    // 双重边界保护：
    //   1. safe_h = height 已由调用者通过 min() 保证不超过 dst 容量
    //   2. dst_check 在写入前再校验索引，作为最后一道防线
    for row in 0..height {
        let src_row_start = row * stride;
        let dst_row_start = row * width;

        // 如果本行起点已超出 dst，立即终止（理论上不应发生，作为安全冗余）
        if dst_row_start >= dst.len() {
            break;
        }

        for col in 0..width {
            let src_idx = src_row_start + col * 4;
            let dst_idx = dst_row_start + col;

            // src / dst 双重越界防护
            if src_idx + 3 >= src.len() || dst_idx >= dst.len() {
                break;
            }

            let b = src[src_idx] as u32; // Blue  — 索引 +0
            let g = src[src_idx + 1] as u32; // Green — 索引 +1
            let r = src[src_idx + 2] as u32; // Red   — 索引 +2
                                             // Alpha 索引 +3，minifb 忽略高字节

            dst[dst_idx] = (r << 16) | (g << 8) | b;
        }
    }
}

// ======================================================================
//  OSD 文字渲染（内嵌 5×7 点阵字库，零外部依赖）
// ======================================================================

/// 在 minifb u32 缓冲区上的指定位置绘制 ASCII 字符串。
///
/// # 参数
/// - `buf`：minifb 像素缓冲区（0x00RRGGBB 格式）
/// - `buf_width`：缓冲区宽度（像素）
/// - `text`：要绘制的 ASCII 文本
/// - `x`：文字左上角 x 坐标
/// - `y`：文字左上角 y 坐标
/// - `color`：文字颜色（0x00RRGGBB）
///
/// 每个字符占 6 列（5 像素字形 + 1 像素间距），7 行高。
/// 先绘制半透明黑色阴影（偏移 +1，+1）再绘制前景色，增强可读性。
#[cfg(target_os = "macos")]
fn draw_text_osd(buf: &mut [u32], buf_width: usize, text: &str, x: usize, y: usize, color: u32) {
    const CHAR_W: usize = 6; // 字符宽度（含右侧 1px 间距）

    for (ci, ch) in text.chars().enumerate() {
        let cx = x + ci * CHAR_W;
        let bitmap = char_bitmap(ch);

        for (row, &bits) in bitmap.iter().enumerate() {
            for col in 0..5usize {
                if bits & (1 << (4 - col)) != 0 {
                    // 先画黑色阴影（+1, +1 偏移），提升对比度
                    let sx = cx + col + 1;
                    let sy = y + row + 1;
                    if let Some(idx) = pixel_idx(sx, sy, buf_width, buf.len()) {
                        // 半透明黑色阴影（直接赋值，避免 alpha 混合开销）
                        buf[idx] = 0x00_10_10_10;
                    }

                    // 再画前景色
                    let px = cx + col;
                    let py = y + row;
                    if let Some(idx) = pixel_idx(px, py, buf_width, buf.len()) {
                        buf[idx] = color;
                    }
                }
            }
        }
    }
}

/// 计算像素在一维缓冲区中的索引，越界则返回 None。
#[cfg(target_os = "macos")]
#[inline]
fn pixel_idx(x: usize, y: usize, width: usize, buf_len: usize) -> Option<usize> {
    let idx = y * width + x;
    if idx < buf_len && x < width {
        Some(idx)
    } else {
        None
    }
}

/// 返回 ASCII 字符的 5×7 点阵位图。
///
/// 每个字符用 7 个 u8 表示，每个 u8 的低 5 位对应一行的 5 个像素列
/// （bit4 = 最左列，bit0 = 最右列）。
///
/// 字库覆盖：数字 0-9、大写字母 A-Z、空格、冒号、小数点、'x'（别名）。
/// 其他字符显示为空心方块 □。
#[cfg(target_os = "macos")]
fn char_bitmap(c: char) -> [u8; 7] {
    match c {
        ' ' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        ':' => [0x00, 0x04, 0x00, 0x00, 0x04, 0x00, 0x00],
        '.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x00],
        // ── 数字 ────────────────────────────────────────────────────────
        '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
        '3' => [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        '5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        '6' => [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
        // ── 大写字母 ────────────────────────────────────────────────────
        'A' => [0x04, 0x0A, 0x11, 0x11, 0x1F, 0x11, 0x11],
        'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        'D' => [0x1E, 0x09, 0x09, 0x09, 0x09, 0x09, 0x1E],
        'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        'G' => [0x0E, 0x11, 0x10, 0x13, 0x11, 0x11, 0x0F],
        'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'I' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        'J' => [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0C],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x11, 0x19, 0x15, 0x13, 0x11, 0x11],
        'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        'Q' => [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
        'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x0A, 0x0A, 0x04],
        'W' => [0x11, 0x11, 0x15, 0x15, 0x15, 0x1B, 0x11],
        'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        'Z' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
        // ── 小写字母（常用）────────────────────────────────────────────
        'x' => [0x00, 0x00, 0x11, 0x0A, 0x04, 0x0A, 0x11],
        // ── 其他字符：显示为空心方块 ────────────────────────────────────
        _ => [0x1F, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1F],
    }
}
