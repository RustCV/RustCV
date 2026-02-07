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
use rustcv_core::traits::Driver;
#[cfg(target_os = "linux")]
use std::time::{Duration, Instant};
#[cfg(target_os = "linux")]
use v4l::video::Capture;

#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() -> Result<()> {
    // 1. 初始化日志，以便看到我们之前埋下的 tracing::info!
    tracing_subscriber::fmt::init();

    println!("=== RustCV V4L2 Backend Demo ===");

    // 2. 实例化驱动
    let driver = V4l2Driver::new();

    // 3. 枚举设备
    let devices = driver.list_devices()?;
    if devices.is_empty() {
        anyhow::bail!("No cameras found! Please plug in a USB camera.");
    }

    println!("Found {} devices:", devices.len());
    for (i, dev) in devices.iter().enumerate() {
        println!(
            "  [{}] {} ({}) - {}",
            i,
            dev.name,
            dev.id,
            dev.bus_info.as_deref().unwrap_or("N/A")
        );
    }

    if let Err(e) = dump_capabilities(&devices[0].id) {
        // 替换为你的设备路径
        eprintln!("Failed to dump caps: {}", e);
    }

    // 默认选择第一个设备
    let target_device = &devices[0];
    println!("\nOpening device: {}", target_device.name);

    let width: usize = 640;
    let height: usize = 480;

    // 4. 构建配置 (智能协商)
    // 我们请求 640x480 @ 30fps，偏好 YUYV 格式
    let config = CameraConfig::new()
        .resolution(width as u32, height as u32, Priority::Required) // 必须是 640x480
        .fps(30, Priority::High) // 尽量 30fps
        .format(FourCC::YUYV, Priority::High); // 偏好 YUYV (方便转 RGB)

    // 5. 打开设备
    // 这里会触发我们在 device.rs 里写的 negotiate_format 逻辑
    let (mut stream, _controls) = driver
        .open(&target_device.id, config)
        .context("Failed to open camera")?;

    // 6. 启动流
    stream.start().await.context("Failed to start stream")?;
    println!("Stream started! Press ESC to exit.");

    // 创建窗口
    // let width = 640;
    // let height = 480;
    let mut window = Window::new(
        "RustCV - Camera Preview",
        width,
        height,
        WindowOptions::default(),
    )?;

    // 用于 FPS 计算
    let mut last_time = Instant::now();
    let mut frame_count = 0;

    // 用于 RGB 显示的缓冲 (minifb 需要 u32 格式: 0x00RRGGBB)
    let mut rgb_buffer: Vec<u32> = vec![0; width * height];

    while window.is_open() && !window.is_key_down(Key::Escape) {
        // 7. 获取下一帧 (零拷贝)
        // frame.data 直接指向内核 mmap 区域
        let frame = stream.next_frame().await?;

        // 8. 简单的 YUYV -> RGB 转换
        // 注意：生产环境应该用 Shader 或 SIMD 做这个，这里仅为演示
        if frame.format == FourCC::YUYV {
            yuyv_to_rgb32(frame.data, &mut rgb_buffer, width, height);
        } else {
            // 如果协商到了 MJPEG，这里暂时无法显示，打印警告
            // (实际项目中需集成 libjpeg-turbo)
            if frame_count % 30 == 0 {
                println!(
                    "Frame format is {:?}, raw display not supported in demo.",
                    frame.format
                );
            }
        }

        // 9. 更新窗口
        window.update_with_buffer(&rgb_buffer, width, height)?;

        // 10. 打印遥测数据 (每秒一次)
        frame_count += 1;
        if last_time.elapsed() >= Duration::from_secs(1) {
            // let fps = frame_count as f64 / last_time.elapsed().as_secs_f64();

            // // 【修改前】这是导致报错的代码：
            // // let latency = Instant::now().duration_since(frame.timestamp.system_synced);

            // // 【修改后】直接获取 Duration 值，不计算 Latency
            // let timestamp_val = frame.timestamp.system_synced;

            // // 读取曝光值 (验证 Controls 模块)
            // let exposure = controls.sensor.get_exposure().unwrap_or(0);

            // // 修改 println! 格式
            // println!(
            //     "FPS: {:.1} | Exp: {} | TS: {:?} | Seq: {}",
            //     fps, exposure, timestamp_val, frame.sequence
            // );

            last_time = Instant::now();
            frame_count = 0;
        }
    }

    // 11. 停止流
    stream.stop().await?;
    println!("Stream stopped.");

    Ok(())
}

/// 辅助函数：将 YUYV (YUV422) 转换为 RGB32 (用于 minifb 显示)
/// 算法：标准 BT.601 转换
#[cfg(target_os = "linux")]
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
    // YUYV 布局: Y0 U0 Y1 V0 (4 bytes 描述 2 pixels)
    // 假设 src 长度足够
    // let num_pixels = width * height;
    let limit = src.len() / 4; // 处理多少组 (2px 一组)

    for i in 0..limit {
        let y0 = src[i * 4] as i32;
        let u = src[i * 4 + 1] as i32 - 128;
        let y1 = src[i * 4 + 2] as i32;
        let v = src[i * 4 + 3] as i32 - 128;

        let c0 = y0 - 16;
        let c1 = y1 - 16;
        let d = u;
        let e = v;

        // Pixel 1
        let r0 = clip((298 * c0 + 409 * e + 128) >> 8);
        let g0 = clip((298 * c0 - 100 * d - 208 * e + 128) >> 8);
        let b0 = clip((298 * c0 + 516 * d + 128) >> 8);

        // Pixel 2
        let r1 = clip((298 * c1 + 409 * e + 128) >> 8);
        let g1 = clip((298 * c1 - 100 * d - 208 * e + 128) >> 8);
        let b1 = clip((298 * c1 + 516 * d + 128) >> 8);

        // 写入 Buffer (0x00RRGGBB)
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

#[cfg(target_os = "linux")]
fn dump_capabilities(dev_path: &str) -> anyhow::Result<()> {
    println!("--- Inspecting capabilities for: {} ---", dev_path);

    // 【关键修复】显式引入枚举和它的变体结构体
    use v4l::framesize::FrameSizeEnum;

    let dev = v4l::Device::with_path(dev_path)?;
    let formats = dev.enum_formats()?;

    for fmt in formats {
        println!("[Format] {} ({})", fmt.fourcc, fmt.description);

        match dev.enum_framesizes(fmt.fourcc) {
            Ok(sizes) => {
                for size in sizes {
                    // 【关键修复】使用引入的 FrameSize 枚举进行匹配
                    match size.size {
                        FrameSizeEnum::Discrete(d) => {
                            println!("    - {}x{}", d.width, d.height);
                        }
                        FrameSizeEnum::Stepwise(s) => {
                            println!(
                                "    - Stepwise: {}x{} to {}x{} (step {}x{})",
                                s.min_width,
                                s.min_height,
                                s.max_width,
                                s.max_height,
                                s.step_width,
                                s.step_height
                            );
                        }
                    }
                }
            }
            Err(e) => println!("    - Failed to get sizes: {}", e),
        }
    }
    println!("----------------------------------------\n");
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    println!("This example is only supported on Linux with V4L2.");
}
