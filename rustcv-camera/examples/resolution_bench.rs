/// Resolution benchmark — compare live FPS at 480p, 720p, and 1080p.
/// 分辨率基准测试 —— 对比 480p、720p、1080p 下的实时 FPS。
///
/// Opens a preview window for each resolution, displays live FPS overlay,
/// then prints a summary comparison table.
///
/// 为每个分辨率打开预览窗口并显示实时 FPS 叠加，
/// 最后打印对比汇总表。
///
/// Run with:
///   cargo run --release --example resolution_bench -p rustcv-camera --features turbojpeg
use std::time::Instant;

use minifb::{Key, Window, WindowOptions};
use rustcv_camera::{CameraConfig, Mat, VideoCapture};

/// Test configuration for a single resolution run.
/// 单次分辨率测试的配置。
struct ResolutionTest {
    width: u32,
    height: u32,
    label: &'static str,
}

fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let tests = [
        ResolutionTest {
            width: 640,
            height: 480,
            label: "480p",
        },
        ResolutionTest {
            width: 1280,
            height: 720,
            label: "720p",
        },
        ResolutionTest {
            width: 1920,
            height: 1080,
            label: "1080p",
        },
    ];

    let mut results: Vec<(&str, u32, u32, f64, String)> = Vec::new();

    for test in &tests {
        println!(
            "\n=== Testing {} ({}x{}) ===",
            test.label, test.width, test.height
        );

        match run_resolution_test(test) {
            Ok((fps, format)) => {
                println!("  Result: {:.1} fps ({})", fps, format);
                results.push((test.label, test.width, test.height, fps, format));
            }
            Err(e) => {
                println!("  Skipped: {}", e);
                results.push((test.label, test.width, test.height, 0.0, "N/A".into()));
            }
        }
    }

    // Print summary table.
    // 打印汇总表。
    println!("\n╔══════════╦════════════════╦══════════╦══════════╗");
    println!("║ Resolution ║ Actual         ║  FPS     ║ Format   ║");
    println!("╠══════════╬════════════════╬══════════╬══════════╣");
    for (label, w, h, fps, fmt) in &results {
        println!(
            "║ {:<8} ║ {:>5}x{:<5}    ║ {:>6.1}   ║ {:<8} ║",
            label, w, h, fps, fmt
        );
    }
    println!("╚══════════╩════════════════╩══════════╩══════════╝");

    Ok(())
}

/// Run a single resolution test with live preview window.
/// 运行单次分辨率测试，带实时预览窗口。
///
/// Captures frames for ~5 seconds (or until ESC), returns average FPS.
/// 采集约 5 秒的帧（或按 ESC 退出），返回平均 FPS。
fn run_resolution_test(
    test: &ResolutionTest,
) -> std::result::Result<(f64, String), Box<dyn std::error::Error>> {
    let config = CameraConfig::new()
        .resolution(test.width, test.height)
        .fps(30);

    let mut cap = VideoCapture::open_with(0, config)?;
    let actual_w = cap.width() as usize;
    let actual_h = cap.height() as usize;
    let format = cap.camera().pixel_format().to_string();

    println!(
        "  Opened: {}x{} {} (requested {}x{})",
        actual_w, actual_h, format, test.width, test.height
    );

    // Create window sized to actual resolution.
    // 创建与实际分辨率匹配的窗口。
    let title = format!("rustcv-camera {} ({}x{})", test.label, actual_w, actual_h);
    let mut window = Window::new(&title, actual_w, actual_h, WindowOptions::default())?;

    let mut frame = Mat::new();
    let mut display_buf: Vec<u32> = vec![0; actual_w * actual_h];

    let start = Instant::now();
    let mut fps_timer = Instant::now();
    let mut fps_count: u32 = 0;
    let mut fps_display: f64 = 0.0;
    let mut total_frames: u64 = 0;

    // Run for ~5 seconds or until ESC.
    // 运行约 5 秒或按 ESC 退出。
    while window.is_open() && !window.is_key_down(Key::Escape) && start.elapsed().as_secs() < 5 {
        if !cap.read(&mut frame)? {
            break;
        }

        bgr_to_u32(frame.data(), &mut display_buf);
        draw_fps_overlay(&mut display_buf, actual_w, fps_display, test.label);
        window.update_with_buffer(&display_buf, actual_w, actual_h)?;

        total_frames += 1;
        fps_count += 1;
        let elapsed = fps_timer.elapsed().as_secs_f64();
        if elapsed >= 1.0 {
            fps_display = fps_count as f64 / elapsed;
            fps_count = 0;
            fps_timer = Instant::now();
        }
    }

    let avg_fps = total_frames as f64 / start.elapsed().as_secs_f64();
    Ok((avg_fps, format))
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

/// Draw FPS and resolution label overlay.
/// 绘制 FPS 和分辨率标签叠加。
fn draw_fps_overlay(buf: &mut [u32], stride: usize, fps: f64, label: &str) {
    let text = format!("{} FPS:{:.1}", label, fps);
    let scale = 2;
    let x0 = 8;
    let y0 = 8;

    // Black background.
    // 黑色背景。
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
                                buf[py * stride + px] = 0x0000FF00; // green
                            }
                        }
                    }
                }
            }
        }
    }
}

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
        'p' => [
            0b00000, 0b00000, 0b11110, 0b10001, 0b11110, 0b10000, 0b10000,
        ],
        _ => [
            0b11111, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11111,
        ],
    }
}
