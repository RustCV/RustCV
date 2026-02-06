#[cfg(target_os = "linux")]
use anyhow::{Context, Result};
#[cfg(target_os = "linux")]
use minifb::{Key, Window, WindowOptions};
#[cfg(target_os = "linux")]
use rustcv_backend_v4l2::V4l2Driver;
#[cfg(target_os = "linux")]
use rustcv_core::builder::{CameraConfig, Priority};
#[cfg(target_os = "linux")]
use rustcv_core::pixel_format::FourCC;
#[cfg(target_os = "linux")]
use rustcv_core::traits::{Driver, Stream};
#[cfg(target_os = "linux")]
use std::sync::{Arc, Mutex};
#[cfg(target_os = "linux")]
use std::time::Duration;

#[cfg(target_os = "linux")]
const WIDTH: usize = 640;
#[cfg(target_os = "linux")]
const HEIGHT: usize = 480;

#[cfg(target_os = "linux")]
// 共享的帧缓冲区，用于主线程渲染
// DoubleBuffer 存放两路画面：[Left Buffer, Right Buffer]
struct SharedBuffer {
    left: Vec<u32>,
    right: Vec<u32>,
    updated_left: bool,
    updated_right: bool,
}

#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() -> Result<()> {
    // 定义画面尺寸

    tracing_subscriber::fmt::init();
    println!("=== RustCV Dual Camera Demo ===");

    let driver = V4l2Driver::new();
    let devices = driver.list_devices()?;

    // 1. 检查是否有足够的设备
    if devices.len() < 2 {
        anyhow::bail!("Need at least 2 cameras! Found: {}", devices.len());
    }

    let dev1_info = &devices[0]; // 左摄
    let dev2_info = &devices[2]; // 右摄

    println!("Cam 1 (Left): {} ({})", dev1_info.name, dev1_info.id);
    println!("Cam 2 (Right): {} ({})", dev2_info.name, dev2_info.id);

    // 2. 配置两个相机 (同样的配置)
    let config = CameraConfig::new()
        .resolution(WIDTH as u32, HEIGHT as u32, Priority::Required)
        .fps(30, Priority::High)
        .format(FourCC::YUYV, Priority::Medium);

    // 3. 分别打开设备
    // 注意：这里必须分别 open，获得两个独立的 stream
    let (mut stream1, _ctrl1) = driver
        .open(&dev1_info.id, config.clone())
        .context("Failed to open Cam 1")?;

    let (mut stream2, _ctrl2) = driver
        .open(&dev2_info.id, config.clone())
        .context("Failed to open Cam 2")?;

    // 4. 准备共享内存 (主线程读，两个采集任务写)
    let shared_buffer = Arc::new(Mutex::new(SharedBuffer {
        left: vec![0; WIDTH * HEIGHT],
        right: vec![0; WIDTH * HEIGHT],
        updated_left: false,
        updated_right: false,
    }));

    // 5. 启动流
    stream1.start().await?;
    stream2.start().await?;

    // 6. 启动采集任务 (Task 1: Left Camera)
    let buf_clone1 = shared_buffer.clone();
    let task1 = tokio::spawn(async move {
        // 获取帧 (零拷贝)
        while let Ok(frame) = stream1.next_frame().await {
            let mut guard = buf_clone1.lock().unwrap();
            // 简单的 YUYV -> RGB 转换
            if frame.format == FourCC::YUYV {
                yuyv_to_rgb32(frame.data, &mut guard.left, WIDTH, HEIGHT);
                guard.updated_left = true;
            }
        }
    });

    // 7. 启动采集任务 (Task 2: Right Camera)
    let buf_clone2 = shared_buffer.clone();
    let task2 = tokio::spawn(async move {
        while let Ok(frame) = stream2.next_frame().await {
            let mut guard = buf_clone2.lock().unwrap();
            if frame.format == FourCC::YUYV {
                yuyv_to_rgb32(frame.data, &mut guard.right, WIDTH, HEIGHT);
                guard.updated_right = true;
            }
        }
    });

    // 8. 主线程：渲染循环
    // 创建一个宽窗口 (640*2 = 1280)
    let mut window = Window::new(
        "RustCV - Dual Camera View",
        WIDTH * 2,
        HEIGHT,
        WindowOptions::default(),
    )?;

    // 最终显示的大 Buffer
    let mut display_buffer: Vec<u32> = vec![0; (WIDTH * 2) * HEIGHT];

    while window.is_open() && !window.is_key_down(Key::Escape) {
        // 锁定共享数据，拷贝到显示 Buffer
        {
            let mut guard = shared_buffer.lock().unwrap();

            // 只有当有新数据时才重新拼合 (简单的优化)
            if guard.updated_left || guard.updated_right {
                combine_buffers(
                    &guard.left,
                    &guard.right,
                    &mut display_buffer,
                    WIDTH,
                    HEIGHT,
                );
                guard.updated_left = false;
                guard.updated_right = false;
            }
        }

        // 刷新窗口
        window.update_with_buffer(&display_buffer, WIDTH * 2, HEIGHT)?;

        // 控制刷新率，避免占用 100% CPU
        // 在实际 GUI 框架 (如 Iced/Slint) 中不需要手动 sleep
        std::thread::sleep(Duration::from_millis(10));
    }

    // 9. 清理
    // Abort 采集任务
    task1.abort();
    task2.abort();
    println!("Exiting...");

    Ok(())
}

#[cfg(target_os = "linux")]
/// 将两个 640x480 的图拼成一个 1280x480 的图
fn combine_buffers(left: &[u32], right: &[u32], dest: &mut [u32], w: usize, h: usize) {
    // 这是一个内存拷贝密集型操作，生产环境建议用 GPU Shader 做
    for y in 0..h {
        let dest_row_start = y * (w * 2);
        let src_row_start = y * w;

        // 拷贝左图的一行
        dest[dest_row_start..dest_row_start + w]
            .copy_from_slice(&left[src_row_start..src_row_start + w]);

        // 拷贝右图的一行
        dest[dest_row_start + w..dest_row_start + 2 * w]
            .copy_from_slice(&right[src_row_start..src_row_start + w]);
    }
}

#[cfg(target_os = "linux")]
// 复用之前的 YUYV 转 RGB 逻辑
fn yuyv_to_rgb32(src: &[u8], dest: &mut [u32], width: usize, height: usize) {
    // 【作用1】安全检查：确保数据长度和分辨率匹配
    // YUYV 是每像素 2 字节，RGB32 是每像素 1 个 u32
    let expected_src_len = width * height * 2;
    let expected_dest_len = width * height;

    if src.len() < expected_src_len || dest.len() < expected_dest_len {
        // 在生产环境中应该返回 Result，这里简单打印错误或直接 panic
        eprintln!(
            "Error: Buffer size mismatch! Expected {} bytes, got {}",
            expected_src_len,
            src.len()
        );
        return;
    }

    let limit = src.len() / 4;
    for i in 0..limit {
        let y0 = src[i * 4] as i32;
        let u = src[i * 4 + 1] as i32 - 128;
        let y1 = src[i * 4 + 2] as i32;
        let v = src[i * 4 + 3] as i32 - 128;

        let c0 = y0 - 16;
        let c1 = y1 - 16;
        let d = u;
        let e = v;

        let r0 = clip((298 * c0 + 409 * e + 128) >> 8);
        let g0 = clip((298 * c0 - 100 * d - 208 * e + 128) >> 8);
        let b0 = clip((298 * c0 + 516 * d + 128) >> 8);

        let r1 = clip((298 * c1 + 409 * e + 128) >> 8);
        let g1 = clip((298 * c1 - 100 * d - 208 * e + 128) >> 8);
        let b1 = clip((298 * c1 + 516 * d + 128) >> 8);

        let idx = i * 2;
        if idx + 1 < dest.len() {
            dest[idx] = (r0 << 16) | (g0 << 8) | b0;
            dest[idx + 1] = (r1 << 16) | (g1 << 8) | b1;
        }
    }
}

#[cfg(target_os = "linux")]
#[inline]
fn clip(val: i32) -> u32 {
    if val < 0 {
        0
    } else if val > 255 {
        255
    } else {
        val as u32
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    println!("This example is only supported on Linux with V4L2.");
}
