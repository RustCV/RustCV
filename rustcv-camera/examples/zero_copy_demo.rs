/// Zero-copy camera demo — accesses raw frame data without any copy.
/// 零拷贝摄像头演示 —— 无任何拷贝地访问原始帧数据。
///
/// This example uses the `Camera` API which provides direct access
/// to the kernel's mmap buffer via `Frame::data()`.
///
/// 此示例使用 `Camera` API，通过 `Frame::data()` 直接访问
/// 内核的 mmap 缓冲区。
///
/// Run with:
///   cargo run --release --example zero_copy_demo -p rustcv-camera
use std::time::Instant;

use rustcv_camera::Camera;

fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    println!("=== rustcv-camera: Zero-Copy Demo ===");

    // Open camera 0 with default settings.
    // 使用默认设置打开 0 号摄像头。
    let mut cam = Camera::open(0)?;
    let config = cam.config();
    println!(
        "Camera opened: {}x{} {:?} ({}fps, {} buffers)",
        config.width, config.height, config.pixel_format, config.fps, config.buffer_count,
    );

    let mut start: Option<Instant> = None;
    let mut count: u64 = 0;
    let mut last_seq: u64 = 0;
    let mut dropped: u64 = 0;

    println!("Capturing... Press Ctrl+C to stop.\n");

    for _ in 0..100 {
        // next_frame() returns Frame<'_> — zero-copy borrow of mmap buffer.
        // The frame MUST be dropped before calling next_frame() again.
        // next_frame() 返回 Frame<'_> —— mmap 缓冲区的零拷贝借用。
        // 帧必须在再次调用 next_frame() 前被 drop。
        let frame = cam.next_frame()?;

        // Start the timer on the first frame to exclude camera startup latency
        // (~0.5–1s on macOS before AVFoundation delivers the first frame).
        // 第一帧到达后才启动计时器，排除摄像头启动延迟（macOS 上约 0.5–1s）。
        if start.is_none() {
            start = Some(Instant::now());
        }

        // Detect dropped frames via sequence number gaps.
        // 通过序号间隔检测丢帧。
        if count > 0 && frame.sequence() > last_seq + 1 {
            let gap = frame.sequence() - last_seq - 1;
            dropped += gap;
        }
        last_seq = frame.sequence();

        // frame.data() is zero-copy — points directly to kernel memory.
        // frame.data() 是零拷贝 —— 直接指向内核内存。
        let data = frame.data();

        if count.is_multiple_of(10) {
            println!(
                "Frame {:3}: {:?} {}x{} | {} bytes | seq={} | ts={}us",
                count,
                frame.pixel_format(),
                frame.width(),
                frame.height(),
                data.len(),
                frame.sequence(),
                frame.timestamp_us(),
            );
        }

        count += 1;
        // frame is dropped here — buffer returned to kernel on next next_frame() call.
        // frame 在此处被 drop —— 缓冲区在下次 next_frame() 调用时归还给内核。
    }

    let elapsed = start.unwrap_or_else(Instant::now).elapsed().as_secs_f64();
    let fps = count as f64 / elapsed;

    println!("\n--- Results ---");
    println!("Frames captured: {}", count);
    println!("Frames dropped:  {}", dropped);
    println!("Elapsed:         {:.2}s", elapsed);
    println!("FPS:             {:.1}", fps);
    println!(
        "Drop rate:       {:.1}%",
        dropped as f64 / (count + dropped) as f64 * 100.0
    );

    Ok(())
}
