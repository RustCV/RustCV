pub mod backend;

use crate::core::mat::Mat;
use crate::internal::runtime;
use anyhow::{anyhow, Result};
use crossbeam_channel::{bounded, Receiver, Sender};
use rustcv_core::builder::CameraConfig;
use rustcv_core::pixel_format::{FourCC, PixelFormat};
use rustcv_core::traits::Stream;

#[cfg(feature = "turbojpeg")]
use turbojpeg::{Decompressor, Image, PixelFormat as TJPixelFormat};

/// 指令：主线程 -> 后台
enum Command {
    NextFrame,
    SetResolution(u32, u32), // 【新增】设置分辨率
    Stop,
}

/// 响应：后台 -> 主线程
enum Response {
    FrameData {
        width: u32,
        height: u32,
        data: Vec<u8>,
        fourcc: u32,
    },
    PropertySet, // 【新增】属性设置成功确认
    Error(String),
    #[allow(dead_code)]
    EndOfStream,
}

pub struct VideoCapture {
    cmd_tx: Sender<Command>,
    res_rx: Receiver<Response>,
    width: i32,
    height: i32,
    is_opened: bool,
}

impl VideoCapture {
    pub fn new(index: u32) -> Result<Self> {
        // 1. 创建驱动 (移动到后台线程前先创建)
        let driver = backend::create_driver()?;

        // 2. 查找设备 ID
        let devices = driver
            .list_devices()
            .map_err(|e| anyhow!("Failed to list devices: {}", e))?;
        let device_id = devices
            .get(index as usize)
            .ok_or_else(|| anyhow!("Camera index {} out of range", index))?
            .id
            .clone();

        // 3. 创建通道
        let (cmd_tx, cmd_rx) = bounded::<Command>(1);
        let (res_tx, res_rx) = bounded::<Response>(1);

        // 4. 【升级】启动后台任务
        // 我们将 driver 和 device_id 移动到后台，让后台全权管理生命周期
        runtime::get_runtime().spawn(async move {
            // 初始配置 (默认)
            let mut current_config = CameraConfig::new();
            // 当前流 (Option，允许为空以便重启)
            let mut current_stream: Option<Box<dyn Stream>> = None;

            // 内部辅助：尝试打开流
            // 这是一个闭包无法捕获 async 引用，所以我们用 macro 或者简单的代码块复用逻辑
            // 这里为了简单，直接在循环外先尝试打开一次
            match driver.open(&device_id, current_config.clone()) {
                Ok((s, _)) => {
                    // 启动流
                    let mut s = s;
                    if let Err(e) = s.start().await {
                        let _ = res_tx.send(Response::Error(format!("Stream start failed: {}", e)));
                        return;
                    }
                    current_stream = Some(s);
                }
                Err(e) => {
                    // 初始打开失败不要紧，后续 NextFrame 会报错，或者允许 SetResolution 修复
                    eprintln!("Warning: Initial open failed: {}", e);
                }
            }

            // 循环处理指令
            while let Ok(cmd) = cmd_rx.recv() {
                match cmd {
                    Command::NextFrame => {
                        if let Some(stream) = current_stream.as_mut() {
                            match stream.next_frame().await {
                                Ok(frame) => {
                                    let data_vec = frame.data.to_vec();
                                    let w = frame.width;
                                    let h = frame.height;
                                    // 提取 FourCC
                                    let fourcc_val: u32 = match frame.format {
                                        PixelFormat::Known(fcc) => fcc.0,
                                        PixelFormat::Unknown(val) => val,
                                    };

                                    let _ = res_tx.send(Response::FrameData {
                                        width: w,
                                        height: h,
                                        data: data_vec,
                                        fourcc: fourcc_val,
                                    });
                                }
                                Err(e) => {
                                    let _ = res_tx.send(Response::Error(e.to_string()));
                                }
                            }
                        } else {
                            let _ = res_tx.send(Response::Error("Camera not opened".into()));
                        }
                    }

                    // 【核心逻辑】设置分辨率 = 热重载
                    Command::SetResolution(w, h) => {
                        // 1. 停止并销毁旧流
                        if let Some(mut stream) = current_stream.take() {
                            let _ = stream.stop().await;
                            // stream 被 Drop，硬件资源释放
                        }

                        // 2. 更新配置
                        current_config = CameraConfig::new().resolution(
                            w,
                            h,
                            rustcv_core::prelude::Priority::Required,
                        );

                        // 3. 重新打开驱动
                        match driver.open(&device_id, current_config.clone()) {
                            Ok((mut s, _)) => {
                                if let Err(e) = s.start().await {
                                    let _ = res_tx
                                        .send(Response::Error(format!("Restart failed: {}", e)));
                                } else {
                                    current_stream = Some(s);
                                    let _ = res_tx.send(Response::PropertySet); // 发送成功信号
                                }
                            }
                            Err(e) => {
                                let _ = res_tx.send(Response::Error(format!(
                                    "Failed to set resolution: {}",
                                    e
                                )));
                            }
                        }
                    }

                    Command::Stop => break,
                }
            }

            // 退出清理
            if let Some(mut stream) = current_stream {
                let _ = stream.stop().await;
            }
        });

        Ok(Self {
            cmd_tx,
            res_rx,
            width: 0,
            height: 0,
            is_opened: true,
        })
    }

    pub fn read(&mut self, mat: &mut Mat) -> Result<bool> {
        if !self.is_opened {
            return Ok(false);
        }
        if self.cmd_tx.send(Command::NextFrame).is_err() {
            return Err(anyhow!("Background worker is dead"));
        }

        let response = self
            .res_rx
            .recv()
            .map_err(|_| anyhow!("Failed to receive response"))?;

        match response {
            Response::FrameData {
                width,
                height,
                data,
                fourcc,
            } => {
                self.width = width as i32;
                self.height = height as i32;

                // 确保 Mat 大小匹配
                let target_len = (width * height * 3) as usize;
                if mat.data.len() != target_len {
                    mat.data = vec![0; target_len];
                }
                mat.rows = height as i32;
                mat.cols = width as i32;
                mat.channels = 3;
                mat.step = (width * 3) as usize;

                let fcc = FourCC(fourcc);
                if fcc == FourCC::YUYV {
                    yuyv_to_bgr(&data, &mut mat.data, width as usize, height as usize);
                } else if fcc == FourCC::MJPEG {
                    // === TurboJPEG v1.4.0 极速解码 ===
                    #[cfg(feature = "turbojpeg")]
                    {
                        // 1. 创建解压器
                        // v1.4.0 API: Decompressor::new() 返回 Result
                        let mut decompressor = Decompressor::new()
                            .map_err(|e| anyhow!("Failed to init TurboJPEG: {}", e))?;

                        // 2. 读取头部信息 (可选，但为了保险起见，获取精确的图像尺寸)
                        let header = decompressor
                            .read_header(&data)
                            .map_err(|e| anyhow!("Failed to read JPEG header: {}", e))?;

                        // 3. 构建 Image 视图，直接指向 Mat 的数据
                        // 这是一个 Zero-Copy 操作，Image 只是 Mat.data 的一个借用封装
                        let image = Image {
                            pixels: mat.data.as_mut_slice(), // 直接写入 Mat
                            width: header.width,             // 图像宽度
                            pitch: mat.step,                 // 关键：对齐步长 (Stride)
                            height: header.height,           // 图像高度
                            format: TJPixelFormat::BGR,      // 直接解码为 BGR，OpenCV 默认格式
                        };

                        // 4. 执行解压 (SIMD 加速)
                        decompressor
                            .decompress(&data, image)
                            .map_err(|e| anyhow!("TurboJPEG decompress failed: {}", e))?;
                    }

                    #[cfg(not(feature = "turbojpeg"))]
                    {
                        // MJPEG decoding
                        if let Ok(img) =
                            image::load_from_memory_with_format(&data, image::ImageFormat::Jpeg)
                        {
                            let rgb = img.to_rgb8();
                            for (i, pixel) in rgb.pixels().enumerate() {
                                // RGB -> BGR
                                mat.data[i * 3] = pixel[2];
                                mat.data[i * 3 + 1] = pixel[1];
                                mat.data[i * 3 + 2] = pixel[0];
                            }
                        } else {
                            return Err(anyhow!("Failed to decode MJPEG"));
                        }
                    }
                } else {
                    // Assume RGB/BGR or Copy
                    if data.len() == target_len {
                        mat.data.copy_from_slice(&data);
                    }
                }
                Ok(true)
            }
            Response::Error(msg) => Err(anyhow!("{}", msg)),
            Response::EndOfStream => Ok(false),
            _ => Err(anyhow!("Unexpected response in read")),
        }
    }

    /// 【新增】设置分辨率
    /// 这是一个同步阻塞调用，会等待后台完成硬件重启
    pub fn set_resolution(&mut self, width: u32, height: u32) -> Result<()> {
        if !self.is_opened {
            return Err(anyhow!("Camera not opened"));
        }

        // 发送指令
        self.cmd_tx
            .send(Command::SetResolution(width, height))
            .map_err(|_| anyhow!("Background worker is dead"))?;

        // 等待确认
        let response = self
            .res_rx
            .recv()
            .map_err(|_| anyhow!("Failed to receive response"))?;

        match response {
            Response::PropertySet => Ok(()), // 成功
            Response::Error(e) => Err(anyhow!("Failed to set resolution: {}", e)),
            _ => Err(anyhow!("Unexpected response")),
        }
    }

    // ... 其他 getter ...
    pub fn is_opened(&self) -> bool {
        self.is_opened
    }
    pub fn get_width(&self) -> i32 {
        self.width
    }
    pub fn get_height(&self) -> i32 {
        self.height
    }
}

impl Drop for VideoCapture {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(Command::Stop);
    }
}

// 辅助：YUYV -> BGR (保留之前的实现)
fn yuyv_to_bgr(src: &[u8], dest: &mut [u8], width: usize, height: usize) {
    let frame_len = width * height * 2;
    if src.len() < frame_len {
        return;
    }
    for i in 0..(width * height / 2) {
        let src_idx = i * 4;
        let dst_idx = i * 6;
        let y0 = src[src_idx] as i32;
        let u = src[src_idx + 1] as i32 - 128;
        let y1 = src[src_idx + 2] as i32;
        let v = src[src_idx + 3] as i32 - 128;
        let c0 = y0 - 16;
        let r0 = (298 * c0 + 409 * v + 128) >> 8;
        let g0 = (298 * c0 - 100 * u - 208 * v + 128) >> 8;
        let b0 = (298 * c0 + 516 * u + 128) >> 8;
        let c1 = y1 - 16;
        let r1 = (298 * c1 + 409 * v + 128) >> 8;
        let g1 = (298 * c1 - 100 * u - 208 * v + 128) >> 8;
        let b1 = (298 * c1 + 516 * u + 128) >> 8;
        dest[dst_idx] = clamp(b0);
        dest[dst_idx + 1] = clamp(g0);
        dest[dst_idx + 2] = clamp(r0);
        dest[dst_idx + 3] = clamp(b1);
        dest[dst_idx + 4] = clamp(g1);
        dest[dst_idx + 5] = clamp(r1);
    }
}

#[inline(always)]
fn clamp(val: i32) -> u8 {
    if val < 0 {
        0
    } else if val > 255 {
        255
    } else {
        val as u8
    }
}
