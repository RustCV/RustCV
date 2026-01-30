// rustcv/examples/demo.rs

use anyhow::Result;
use rustcv::{
    highgui,    // 窗口显示
    imgproc,    // 绘图工具
    prelude::*, // 自动引入 VideoCapture, Mat 等
};
use std::time::Instant;

fn main() -> Result<()> {
    // 1. 打开摄像头 (索引 0)
    // 底层会自动启动后台线程和 Tokio Runtime
    println!("Opening camera...");
    let mut cap = VideoCapture::new(0)?;

    // 这一行会触发：停止流 -> 重置参数 -> 重新打开 -> 启动流
    println!("Setting resolution to 640x480...");
    if let Err(e) = cap.set_resolution(640, 480) {
        eprintln!("Warning: Failed to set resolution: {}. Using default.", e);
    } else {
        println!("Resolution set successfully!");
    }

    if !cap.is_opened() {
        eprintln!("Error: Could not open camera");
        return Ok(());
    }

    // 2. 预分配 Mat (为了 Buffer Swapping 优化)
    // 实际上首次 read 会自动处理大小，这里创建一个空的即可
    let mut frame = Mat::empty();

    // FPS 计算器
    let mut last_time = Instant::now();
    let mut frame_count = 0;
    let mut fps = 0.0;

    println!("Start capturing... Press ESC or Q to exit.");

    // 3. 主循环 (经典的 OpenCV 风格)
    while cap.read(&mut frame)? {
        if frame.is_empty() {
            continue;
        }

        // --- 图像处理 (In-place 修改) ---

        // 模拟人脸检测框 (画一个静态的绿框)
        let rect = imgproc::Rect::new(200, 150, 240, 240);
        imgproc::rectangle(
            &mut frame,
            rect,
            imgproc::Scalar::new(0, 255, 0), // Green (BGR)
            2,
        );

        // 计算 FPS
        frame_count += 1;
        if frame_count % 10 == 0 {
            let now = Instant::now();
            let duration = now.duration_since(last_time).as_secs_f64();
            fps = 10.0 / duration;
            last_time = now;
            frame_count = 0;
        }

        // 绘制 FPS 文字 (红色)
        let fps_text = format!("FPS: {:.1}  Res: {}x{}", fps, frame.cols, frame.rows);
        imgproc::put_text(
            &mut frame,
            &fps_text,
            imgproc::Point::new(10, 30),
            1.0,                             // Font scale
            imgproc::Scalar::new(0, 0, 255), // Red
        );

        // --- 显示 (跨平台 GUI) ---
        highgui::imshow("RustCV Camera", &frame)?;

        // --- 按键控制 ---
        let key = highgui::wait_key(1)?;
        if key == 27 || key == 113 {
            // ESC or 'q'
            println!("Exiting...");
            break;
        }
    }

    // 4. 清理 (Drop 会自动处理，但显式调用更规范)
    highgui::destroy_all_windows()?;

    Ok(())
}
