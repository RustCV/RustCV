use anyhow::Result;

#[cfg(target_os = "windows")]
mod windows_impl {
    use anyhow::{Context, Result};
    use minifb::{Key, Window, WindowOptions};
    use std::time::{Duration, Instant};
    use windows::Win32::System::Com::*;

    use rustcv_backend_msmf::MsmfDriver;
    use rustcv_core::builder::{CameraConfig, Priority};
    use rustcv_core::pixel_format::FourCC;
    use rustcv_core::traits::Driver;

    pub async fn main_body() -> Result<()> {
        tracing_subscriber::fmt::init();

        println!("=== RustCV MSMF Backend Demo ===");

        let driver = MsmfDriver::new();

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
            eprintln!("Failed to dump caps: {}", e);
        }

        let target_device = &devices[0];
        println!("\nOpening device: {}", target_device.name);

        let width: usize = 640;
        let height: usize = 480;

        let config = CameraConfig::new()
            .resolution(width as u32, height as u32, Priority::Required)
            .fps(30, Priority::High)
            .format(FourCC::YUYV, Priority::High);

        let (mut stream, _controls) = driver
            .open(&target_device.id, config)
            .context("Failed to open camera")?;

        stream.start().await.context("Failed to start stream")?;
        println!("Stream started! Press ESC to exit.");

        let mut window = Window::new(
            "RustCV - Camera Preview",
            width,
            height,
            WindowOptions::default(),
        )?;

        let mut last_time = Instant::now();
        let mut frame_count = 0;

        let mut rgb_buffer: Vec<u32> = vec![0; width * height];

        while window.is_open() && !window.is_key_down(Key::Escape) {
            let frame = stream.next_frame().await?;

            if frame.format == FourCC::YUYV {
                yuyv_to_rgb32(frame.data, &mut rgb_buffer, width, height);
            } else {
                if frame_count % 30 == 0 {
                    println!(
                        "Frame format is {:?}, raw display not supported in demo.",
                        frame.format
                    );
                }
            }

            window.update_with_buffer(&rgb_buffer, width, height)?;

            frame_count += 1;
            if last_time.elapsed() >= Duration::from_secs(1) {
                let fps = frame_count as f64 / last_time.elapsed().as_secs_f64();

                let exposure = _controls.sensor.get_exposure().unwrap_or(0);

                println!(
                    "FPS: {:.1} | Exp: {} | Seq: {}",
                    fps, exposure, frame.sequence
                );

                last_time = Instant::now();
                frame_count = 0;
            }
        }

        stream.stop().await?;
        println!("Stream stopped.");

        unsafe {
            CoUninitialize();
        }

        Ok(())
    }

    fn dump_capabilities(dev_path: &str) -> anyhow::Result<()> {
        println!("--- Inspecting capabilities for: {} ---", dev_path);

        let driver = MsmfDriver::new();
        let devices = driver.list_devices()?;

        if let Some(dev) = devices.iter().find(|d| d.id == dev_path) {
            println!("Device: {} ({})", dev.name, dev.id);
            println!("Backend: {}", dev.backend);
            println!("Bus Info: {}", dev.bus_info.as_deref().unwrap_or("N/A"));
        } else {
            println!("Device not found: {}", dev_path);
        }

        println!("----------------------------------------\n");
        Ok(())
    }

    fn yuyv_to_rgb32(src: &[u8], dest: &mut [u32], width: usize, height: usize) {
        let expected_src_len = width * height * 2;
        let expected_dest_len = width * height;

        if src.len() < expected_src_len || dest.len() < expected_dest_len {
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
}

#[tokio::main]
async fn main() -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        windows_impl::main_body().await
    }
    #[cfg(not(target_os = "windows"))]
    {
        println!("This example is only available on Windows.");
        Ok(())
    }
}
