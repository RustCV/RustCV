
#[cfg(target_os = "linux")]
// 图像参数
const WIDTH: u32 = 640;
#[cfg(target_os = "linux")]
const HEIGHT: u32 = 480;

#[cfg(target_os = "linux")]
// 应用状态：保存广播通道的发送端
#[derive(Clone)]
struct AppState {
    // 使用 broadcast channel，支持多个浏览器同时观看
    // 发送的是 JPEG 字节流
    tx: broadcast::Sender<Bytes>,
}

#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() -> Result<()> {
    use anyhow::{Context, Result};
    use axum::{
        body::Body,
        extract::State,
        response::{IntoResponse, Response},
        routing::get,
        Router,
    };
    use bytes::Bytes;
    use futures::StreamExt;
    use std::{net::SocketAddr, time::Duration};
    use tokio::sync::broadcast;

    use rustcv_backend_v4l2::V4l2Driver;
    use rustcv_core::builder::{CameraConfig, Priority};
    use rustcv_core::pixel_format::FourCC;
    use rustcv_core::traits::{Driver, Stream};

    tracing_subscriber::fmt::init();
    println!("=== RustCV Web Streaming Demo ===");

    // 1. 初始化摄像头
    let driver = V4l2Driver::new();
    let devices = driver.list_devices()?;
    if devices.is_empty() {
        anyhow::bail!("No cameras found!");
    }
    // 默认选第 2 个设备 (避开 IR 摄像头，根据你的情况调整索引)
    let dev_idx = if devices.len() > 1 { 2 } else { 0 };
    let device_info = &devices[dev_idx];
    println!("Using camera: {}", device_info.name);

    let config = CameraConfig::new()
        .resolution(WIDTH, HEIGHT, Priority::Required)
        .format(FourCC::YUYV, Priority::High)
        .fps(30, Priority::Medium);

    let (mut stream, _ctrl) = driver
        .open(&device_info.id, config)
        .context("Failed to open camera")?;
    stream.start().await?;

    // 2. 创建广播通道 (容量 16 帧，满了覆盖旧的)
    let (tx, _rx) = broadcast::channel::<Bytes>(16);
    let state = AppState { tx: tx.clone() };

    // 3. 启动后台采集任务 (Producer)
    let tx_clone = tx.clone();
    tokio::spawn(async move {
        println!("Camera capture task started...");
        loop {
            match stream.next_frame().await {
                Ok(frame) => {
                    // 仅处理 YUYV 格式
                    if frame.format == FourCC::YUYV {
                        // YUYV -> RGB -> JPEG
                        // 这一步是 CPU 密集型的，生产环境建议放在 spawn_blocking 里
                        // 或者使用硬件 JPEG 编码器
                        if let Ok(jpeg_bytes) = encode_frame_to_jpeg(frame.data, WIDTH, HEIGHT) {
                            // 广播给所有连接的浏览器
                            // 如果没有浏览器连接，send 会失败，我们要忽略这个错误
                            let _ = tx_clone.send(Bytes::from(jpeg_bytes));
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Capture error: {}", e);
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        }
    });

    // 4. 启动 Web 服务器
    let app = Router::new()
        .route("/", get(index_page))
        .route("/stream", get(stream_handler))
        .with_state(state);

    // 监听 0.0.0.0 以便局域网访问
    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    println!("Listening on http://0.0.0.0:3000");
    println!("Check your LAN IP (e.g., http://192.168.1.x:3000) to view on other devices.");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(target_os = "linux")]
/// 首页 HTML
async fn index_page() -> impl IntoResponse {
    axum::response::Html(
        r#"
        <!DOCTYPE html>
        <html>
        <head><title>RustCV DORA Stream</title></head>
        <body style="background: #111; color: #eee; text-align: center;">
            <h1>DORA Robot Vision Feed</h1>
            <p>Live MJPEG Stream from RustCV</p>
            <div style="margin: 20px auto; border: 2px solid #444; display: inline-block;">
                <img src="/stream" width="640" height="480" />
            </div>
            <p style="color: #aaa;">Latency: Ultra Low</p>
        </body>
        </html>
        "#,
    )
}

#[cfg(target_os = "linux")]
/// 视频流处理函数 (MJPEG)
async fn stream_handler(State(state): State<AppState>) -> Response {
    // 1. 创建订阅流
    let rx = state.tx.subscribe();

    // 2. 构造 MJPEG 数据流
    let stream = tokio_stream::wrappers::BroadcastStream::new(rx)
        .filter_map(|result| async move {
            // 过滤掉 Lagged 错误
            result.ok()
        })
        .map(|bytes| {
            // 构造 MJPEG 帧
            let header = format!(
                "--frame\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\n\r\n",
                bytes.len()
            );

            let mut full_frame = Vec::with_capacity(header.len() + bytes.len() + 2);
            full_frame.extend_from_slice(header.as_bytes());
            full_frame.extend_from_slice(&bytes);
            full_frame.extend_from_slice(b"\r\n");

            Ok::<_, std::io::Error>(Bytes::from(full_frame))
        });

    // 3. 将流转换为 Body
    let body = Body::from_stream(stream);

    // 4. 将 Body 转换为 Response
    let mut response = body.into_response();

    // 5. 【修正】直接在 response 上修改 Header，而不是用 .map()
    response.headers_mut().insert(
        "Content-Type",
        "multipart/x-mixed-replace; boundary=frame".parse().unwrap(),
    );

    response
}

#[cfg(target_os = "linux")]
/// 辅助函数：YUYV -> JPEG
/// 注意：这里的性能不是最优的，仅做演示。
/// 生产环境应直接使用 libjpeg-turbo 的 C 绑定或 SIMD 优化的库。
fn encode_frame_to_jpeg(yuyv_data: &[u8], width: u32, height: u32) -> Result<Vec<u8>> {
    // 1. YUYV -> RGB (复用之前的逻辑)
    let mut rgb_buffer = vec![0u8; (width * height * 3) as usize];
    yuyv_to_rgb8(yuyv_data, &mut rgb_buffer);

    // 2. RGB -> JPEG (使用 image crate)
    let mut jpeg_buffer = Vec::new();
    let img_buffer = image::RgbImage::from_raw(width, height, rgb_buffer)
        .ok_or_else(|| anyhow::anyhow!("Failed to create image buffer"))?;

    // 使用 JpegEncoder 写入内存，质量设为 75
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_buffer, 75);
    encoder.encode_image(&img_buffer)?;

    Ok(jpeg_buffer)
}

#[cfg(target_os = "linux")]
// 专门为 image crate 优化的 YUYV -> RGB8 (R,G,B, R,G,B...)
fn yuyv_to_rgb8(src: &[u8], dest: &mut [u8]) {
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

        // Pixel 1
        let r0 = clip((298 * c0 + 409 * e + 128) >> 8);
        let g0 = clip((298 * c0 - 100 * d - 208 * e + 128) >> 8);
        let b0 = clip((298 * c0 + 516 * d + 128) >> 8);

        // Pixel 2
        let r1 = clip((298 * c1 + 409 * e + 128) >> 8);
        let g1 = clip((298 * c1 - 100 * d - 208 * e + 128) >> 8);
        let b1 = clip((298 * c1 + 516 * d + 128) >> 8);

        // 写入 RGB8 格式 (3 bytes per pixel)
        let idx = i * 6;
        if idx + 5 < dest.len() {
            dest[idx] = r0;
            dest[idx + 1] = g0;
            dest[idx + 2] = b0;
            dest[idx + 3] = r1;
            dest[idx + 4] = g1;
            dest[idx + 5] = b1;
        }
    }
}

#[cfg(target_os = "linux")]
#[inline]
fn clip(val: i32) -> u8 {
    if val < 0 {
        0
    } else if val > 255 {
        255
    } else {
        val as u8
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    println!("This example is only supported on Linux with V4L2.");
}