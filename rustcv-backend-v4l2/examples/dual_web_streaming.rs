
#[cfg(target_os = "linux")]
// 图像参数
const WIDTH: u32 = 640;
#[cfg(target_os = "linux")]
const HEIGHT: u32 = 480;

#[cfg(target_os = "linux")]
// 应用状态：保存两个摄像头的广播通道
#[derive(Clone)]
struct AppState {
    tx_left: broadcast::Sender<Bytes>,
    tx_right: broadcast::Sender<Bytes>,
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
    println!("=== RustCV Dual Web Streaming Demo ===");

    // 1. 初始化驱动和设备
    let driver = V4l2Driver::new();
    let devices = driver.list_devices()?;

    // 简单的设备选择逻辑：需要至少 2 个设备
    if devices.len() < 2 {
        anyhow::bail!("Need at least 2 cameras! Found: {}", devices.len());
    }

    // 假设索引 0 和 1 是我们需要的一对摄像头
    // (实际项目中可能需要通过 device.name 或 bus_info 来精确匹配)
    let dev_left_info = &devices[0];
    let dev_right_info = &devices[2];

    println!("Left Camera: {}", dev_left_info.name);
    println!("Right Camera: {}", dev_right_info.name);

    let config = CameraConfig::new()
        .resolution(WIDTH, HEIGHT, Priority::Required)
        .format(FourCC::YUYV, Priority::High)
        .fps(30, Priority::Medium);

    // 2. 分别打开两个摄像头
    // 注意：open 返回的是 (Stream, Control)，这里我们只用 Stream
    let (stream_left, _) = driver
        .open(&dev_left_info.id, config.clone())
        .context("Failed to open Left Camera")?;

    let (stream_right, _) = driver
        .open(&dev_right_info.id, config.clone())
        .context("Failed to open Right Camera")?;

    // 3. 创建两个广播通道
    let (tx_left, _) = broadcast::channel::<Bytes>(8); // 缓冲 8 帧
    let (tx_right, _) = broadcast::channel::<Bytes>(8);

    let state = AppState {
        tx_left: tx_left.clone(),
        tx_right: tx_right.clone(),
    };

    // 4. 启动采集任务 (Producers)
    // 启动左摄任务
    spawn_camera_producer(stream_left, tx_left, "Left");
    // 启动右摄任务
    spawn_camera_producer(stream_right, tx_right, "Right");

    // 5. 启动 Web 服务器
    let app = Router::new()
        .route("/", get(index_page))
        .route("/stream_left", get(handle_left_stream))
        .route("/stream_right", get(handle_right_stream))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    println!("Listening on http://0.0.0.0:3000");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(target_os = "linux")]
/// 辅助函数：启动一个摄像头的采集、编码、广播循环
fn spawn_camera_producer<S>(mut stream: S, tx: broadcast::Sender<Bytes>, name: &'static str)
where
    S: Stream + Send + 'static,
{
    tokio::spawn(async move {
        println!("[{}] Capture started...", name);
        if let Err(e) = stream.start().await {
            eprintln!("[{}] Failed to start stream: {}", name, e);
            return;
        }

        loop {
            // 1. 获取帧 (此时 stream 被借用)
            // 使用 match 确保 frame 的生命周期限制在代码块内
            let data_owned = match stream.next_frame().await {
                Ok(frame) => {
                    if frame.format == FourCC::YUYV {
                        // 【关键步骤】将数据拷贝到 Owned Vec
                        // 这样我们就不再依赖 frame (也就解除了对 stream 的借用)
                        Some(frame.data.to_vec())
                    } else {
                        None
                    }
                    // frame 在这里离开作用域，stream 的借用自动解除！
                }
                Err(e) => {
                    eprintln!("[{}] Capture error: {}", name, e);
                    // 出错时稍微等待，防止死循环刷屏
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    None
                }
            };

            // 2. 如果拿到了数据，在后台进行 JPEG 编码
            if let Some(yuyv_data) = data_owned {
                let tx_clone = tx.clone();

                // 使用 spawn_blocking 将 CPU 密集型任务移出异步运行时
                // 注意：这里我们传入的是 yuyv_data (Vec<u8>)，它是完全独立的
                tokio::task::spawn_blocking(move || {
                    // 编码过程不会阻塞摄像头采集下一帧
                    if let Ok(jpeg_bytes) = encode_frame_to_jpeg(&yuyv_data, WIDTH, HEIGHT) {
                        let _ = tx_clone.send(Bytes::from(jpeg_bytes));
                    }
                });
            }

            // 循环回到顶部，stream 现在是自由的，可以再次调用 next_frame()
        }
    });
}

#[cfg(target_os = "linux")]
/// 首页 HTML：双屏显示
async fn index_page() -> impl IntoResponse {
    axum::response::Html(
        r#"
        <!DOCTYPE html>
        <html>
        <head>
            <title>DORA Dual Vision</title>
            <style>
                body { background: #222; color: #eee; font-family: sans-serif; text-align: center; }
                .container { display: flex; justify-content: center; gap: 20px; flex-wrap: wrap; }
                .cam-box { border: 2px solid #555; background: #000; padding: 5px; }
                h2 { margin: 10px 0; font-size: 1.2rem; color: #aaa; }
                img { width: 640px; height: 480px; display: block; }
            </style>
        </head>
        <body>
            <h1>DORA Robot - Dual Vision Feed</h1>
            <div class="container">
                <div class="cam-box">
                    <h2>Left Camera</h2>
                    <img src="/stream_left" />
                </div>
                <div class="cam-box">
                    <h2>Right Camera</h2>
                    <img src="/stream_right" />
                </div>
            </div>
            <p style="color: #666; margin-top: 20px;">Powered by RustCV & Axum</p>
        </body>
        </html>
        "#,
    )
}

#[cfg(target_os = "linux")]
/// 处理器：左摄流
async fn handle_left_stream(State(state): State<AppState>) -> Response {
    mjpeg_stream_response(state.tx_left)
}

#[cfg(target_os = "linux")]
/// 处理器：右摄流
async fn handle_right_stream(State(state): State<AppState>) -> Response {
    mjpeg_stream_response(state.tx_right)
}

#[cfg(target_os = "linux")]
/// 通用 MJPEG 响应构造器
fn mjpeg_stream_response(tx: broadcast::Sender<Bytes>) -> Response {
    let rx = tx.subscribe();

    let stream = tokio_stream::wrappers::BroadcastStream::new(rx)
        .filter_map(|result| async move { result.ok() })
        .map(|bytes| {
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

    let body = Body::from_stream(stream);
    let mut response = body.into_response();

    response.headers_mut().insert(
        "Content-Type",
        "multipart/x-mixed-replace; boundary=frame".parse().unwrap(),
    );
    response
}

#[cfg(target_os = "linux")]
// --- 图像编码逻辑 (与之前相同) ---
fn encode_frame_to_jpeg(yuyv_data: &[u8], width: u32, height: u32) -> Result<Vec<u8>> {
    // 1. YUYV -> RGB
    // 这里为了演示方便，每次都分配新内存。生产环境请务必优化！
    let mut rgb_buffer = vec![0u8; (width * height * 3) as usize];
    yuyv_to_rgb8(yuyv_data, &mut rgb_buffer);

    // 2. RGB -> JPEG
    let mut jpeg_buffer = Vec::new();
    let img_buffer = image::RgbImage::from_raw(width, height, rgb_buffer)
        .ok_or_else(|| anyhow::anyhow!("Failed to create image buffer"))?;

    // 质量调低一点以保证双流流畅度
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_buffer, 60);
    encoder.encode_image(&img_buffer)?;

    Ok(jpeg_buffer)
}

#[cfg(target_os = "linux")]
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

        let r0 = clip((298 * c0 + 409 * e + 128) >> 8);
        let g0 = clip((298 * c0 - 100 * d - 208 * e + 128) >> 8);
        let b0 = clip((298 * c0 + 516 * d + 128) >> 8);

        let r1 = clip((298 * c1 + 409 * e + 128) >> 8);
        let g1 = clip((298 * c1 - 100 * d - 208 * e + 128) >> 8);
        let b1 = clip((298 * c1 + 516 * d + 128) >> 8);

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