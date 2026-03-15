/// Camera preview with live FPS overlay — using minifb window.
/// 带实时 FPS 叠加显示的摄像头预览 —— 使用 minifb 窗口。
///
/// Run with:
///   cargo run --release --example camera_demo -p rustcv-camera --features turbojpeg
use std::time::Instant;

use minifb::{Key, Window, WindowOptions};
use rustcv_camera::{Mat, VideoCapture};

fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    println!("=== rustcv-camera: Camera Preview ===");

    let mut cap = VideoCapture::open(0)?;
    let width = cap.width() as usize;
    let height = cap.height() as usize;
    println!("Camera opened: {}x{}", width, height);

    // Create display window.
    // 创建显示窗口。
    let mut window = Window::new("rustcv-camera", width, height, WindowOptions::default())?;

    // Pre-allocate buffers — reused every frame, zero allocation in the hot loop.
    // 预分配缓冲区 —— 每帧复用，热循环中零分配。
    let mut frame = Mat::new();
    let mut display_buf: Vec<u32> = vec![0; width * height];

    // FPS calculation.
    // FPS 计算。
    let mut fps_timer = Instant::now();
    let mut fps_count: u32 = 0;
    let mut fps_display: f64 = 0.0;

    println!("Press ESC to exit.");

    while window.is_open() && !window.is_key_down(Key::Escape) {
        if !cap.read(&mut frame)? {
            break;
        }

        // Convert BGR Mat → u32 display buffer (0x00RRGGBB for minifb).
        // 将 BGR Mat 转换为 u32 显示缓冲区（minifb 使用 0x00RRGGBB 格式）。
        bgr_to_u32(frame.data(), &mut display_buf);

        // Draw FPS text as simple digit overlay in top-left corner.
        // 在左上角绘制简单的 FPS 数字叠加。
        draw_fps_overlay(&mut display_buf, width, fps_display);

        // Update window.
        // 更新窗口。
        window.update_with_buffer(&display_buf, width, height)?;

        // Update FPS counter every second.
        // 每秒更新一次 FPS 计数。
        fps_count += 1;
        let elapsed = fps_timer.elapsed().as_secs_f64();
        if elapsed >= 1.0 {
            fps_display = fps_count as f64 / elapsed;
            fps_count = 0;
            fps_timer = Instant::now();
        }
    }

    println!("Done.");
    Ok(())
}

/// Convert BGR byte slice to u32 display buffer (0x00RRGGBB).
/// 将 BGR 字节切片转换为 u32 显示缓冲区（0x00RRGGBB）。
fn bgr_to_u32(bgr: &[u8], buf: &mut [u32]) {
    for (pixel, chunk) in buf.iter_mut().zip(bgr.chunks_exact(3)) {
        let b = chunk[0] as u32;
        let g = chunk[1] as u32;
        let r = chunk[2] as u32;
        *pixel = (r << 16) | (g << 8) | b;
    }
}

/// Draw FPS value as white pixels in the top-left corner.
/// 在左上角用白色像素绘制 FPS 数值。
///
/// Uses a minimal 5x7 bitmap font — no external font dependencies.
/// 使用最小的 5x7 位图字体 —— 无外部字体依赖。
fn draw_fps_overlay(buf: &mut [u32], stride: usize, fps: f64) {
    let text = format!("FPS: {:.1}", fps);
    let scale = 2; // 2x scale for visibility / 2 倍放大以便可见
    let x0 = 8;
    let y0 = 8;

    // Draw black background rectangle.
    // 绘制黑色背景矩形。
    let text_w = text.len() * 6 * scale;
    let text_h = 7 * scale;
    for dy in 0..text_h + 4 {
        for dx in 0..text_w + 8 {
            let px = x0 + dx - 2;
            let py = y0 + dy - 2;
            if py < buf.len() / stride && px < stride {
                buf[py * stride + px] = 0x00000000;
            }
        }
    }

    // Draw each character.
    // 绘制每个字符。
    for (ci, ch) in text.chars().enumerate() {
        let glyph = char_glyph(ch);
        for (row, &glyph_row) in glyph.iter().enumerate() {
            for col in 0..5 {
                if glyph_row & (1 << (4 - col)) != 0 {
                    for sy in 0..scale {
                        for sx in 0..scale {
                            let px = x0 + ci * 6 * scale + col * scale + sx;
                            let py = y0 + row * scale + sy;
                            if py < buf.len() / stride && px < stride {
                                buf[py * stride + px] = 0x00FFFFFF; // white
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Minimal 5x7 bitmap font glyph for a character.
/// 字符的最小 5x7 位图字体字形。
///
/// Each element is a row bitmask (5 bits used, MSB = leftmost pixel).
/// 每个元素是一行的位掩码（使用 5 位，MSB = 最左像素）。
fn char_glyph(ch: char) -> [u8; 7] {
    match ch {
        '0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        '3' => [
            0b01110, 0b10001, 0b00001, 0b00110, 0b00001, 0b10001, 0b01110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110,
        ],
        '6' => [
            0b01110, 0b10000, 0b11110, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110,
        ],
        'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'S' => [
            0b01110, 0b10001, 0b10000, 0b01110, 0b00001, 0b10001, 0b01110,
        ],
        ':' => [
            0b00000, 0b00100, 0b00000, 0b00000, 0b00100, 0b00000, 0b00000,
        ],
        '.' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00100,
        ],
        ' ' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000,
        ],
        _ => [
            0b11111, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11111,
        ], // box for unknown
    }
}
