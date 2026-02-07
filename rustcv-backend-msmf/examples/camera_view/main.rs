use anyhow::Result;

mod convert;

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

    use crate::convert::{is_format_supported, nv12_to_rgb32, yuyv_to_rgb32};

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

        let config = CameraConfig::new()
            .resolution(640, 480, Priority::Required)
            .fps(30, Priority::High)
            .format(FourCC::YUYV, Priority::High);

        let (mut stream, _controls) = driver
            .open(&target_device.id, config)
            .context("Failed to open camera")?;

        stream.start().await.context("Failed to start stream")?;
        println!("Stream started! Press ESC to exit.");

        let first_frame = stream.next_frame().await?;
        let frame_width = first_frame.width as usize;
        let frame_height = first_frame.height as usize;

        let mut window = Window::new(
            "RustCV - Camera Preview",
            frame_width,
            frame_height,
            WindowOptions::default(),
        )?;

        let mut last_time = Instant::now();
        let mut frame_times: Vec<Instant> = Vec::with_capacity(60);
        let start_time = Instant::now();

        let mut rgb_buffer: Vec<u32> = vec![0; frame_width * frame_height];

        let mut frame = first_frame;

        loop {
            if !window.is_open() || window.is_key_down(Key::Escape) {
                break;
            }
            if start_time.elapsed() >= Duration::from_secs(60) {
                println!("Time limit reached (60 seconds). Exiting...");
                break;
            }

            frame_times.push(Instant::now());

            if frame.format == FourCC::YUYV {
                yuyv_to_rgb32(
                    frame.data,
                    &mut rgb_buffer,
                    frame.width as usize,
                    frame.height as usize,
                    frame.stride,
                );
            } else if frame.format == FourCC::NV12 {
                nv12_to_rgb32(
                    frame.data,
                    &mut rgb_buffer,
                    frame.width as usize,
                    frame.height as usize,
                    frame.stride,
                );
            } else if frame_times.len().is_multiple_of(30) {
                println!(
                    "Frame format is {:?}, supported: {}",
                    frame.format,
                    is_format_supported(frame.format)
                );
            }

            window.update_with_buffer(&rgb_buffer, frame_width, frame_height)?;

            if last_time.elapsed() >= Duration::from_secs(1) {
                let elapsed = last_time.elapsed().as_secs_f64();
                let fps = frame_times.len() as f64 / elapsed.max(0.001);

                let exposure = _controls.sensor.get_exposure().unwrap_or(0);

                println!(
                    "FPS: {:.1} | Exp: {} | Seq: {}",
                    fps, exposure, frame.sequence
                );

                last_time = Instant::now();
                frame_times.clear();
            }

            frame = stream.next_frame().await?;
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
