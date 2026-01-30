pub mod backend;

use crate::core::mat::Mat;
use crate::internal::runtime;
use anyhow::{anyhow, Result};
use crossbeam_channel::{bounded, Receiver, Sender};
use rustcv_core::builder::CameraConfig;
use rustcv_core::traits::Stream;
// use std::sync::Arc;

/// 指令：主线程发送给后台 Worker 的命令
enum Command {
    /// 请求下一帧
    NextFrame,
    /// 释放资源（可选，因为 Drop 会自动断开 channel）
    Stop,
}

/// 响应：后台 Worker 发回的数据
enum Response {
    /// 成功获取一帧数据 (宽度, 高度, 原始数据)
    FrameData {
        width: u32,
        height: u32,
        data: Vec<u8>, // 所有权数据
    },
    /// 发生错误
    Error(String),
    /// 流结束
    EndOfStream,
}

/// 经典的 OpenCV 风格视频捕获类
pub struct VideoCapture {
    // 发送指令的通道
    cmd_tx: Sender<Command>,
    // 接收数据的通道
    res_rx: Receiver<Response>,
    // 记录属性
    width: i32,
    height: i32,
    is_opened: bool,
}

impl VideoCapture {
    /// 打开摄像头设备 (例如 index=0)
    pub fn new(index: u32) -> Result<Self> {
        // 1. 获取驱动
        let driver = backend::create_driver()?;

        // 2. 转换 index 为设备 ID 字符串 (简单处理：假设列表顺序即 index)
        let devices = driver
            .list_devices()
            .map_err(|e| anyhow!("Failed to list devices: {}", e))?;

        let device_id = devices
            .get(index as usize)
            .ok_or_else(|| anyhow!("Camera index {} out of range", index))?
            .id
            .clone();

        // 3. 配置参数 (默认 VGA，后续通过 set 调整)
        let config = CameraConfig::new(); // 使用默认配置

        // 4. 打开流 (这一步可能涉及 IO，但通常很快)
        // 注意：这里是在主线程打开 Stream，随后我们需要把它 move 到后台线程
        let (mut stream, _controls) = driver
            .open(&device_id, config)
            .map_err(|e| anyhow!("Failed to open camera: {}", e))?;

        // 5. 创建同步通道 (Rendezvous channel，容量为0或1，保证背压)
        // 主线程 -> 后台
        let (cmd_tx, cmd_rx) = bounded::<Command>(1);
        // 后台 -> 主线程
        let (res_tx, res_rx) = bounded::<Response>(1);

        // 6. 【关键魔法】启动后台异步任务
        // 我们不等待这个任务结束，它会在后台一直跑，直到 VideoCapture 被 Drop
        runtime::get_runtime().spawn(async move {
            // 在后台先启动流
            if let Err(e) = stream.start().await {
                let _ = res_tx.send(Response::Error(format!("Stream start failed: {}", e)));
                return;
            }

            // 循环等待指令
            while let Ok(cmd) = cmd_rx.recv() {
                match cmd {
                    Command::NextFrame => {
                        match stream.next_frame().await {
                            Ok(frame) => {
                                // 【Buffer Swapping 基础】
                                // 我们必须在这里把 Frame<'a> 的数据拷贝到 Owned Vec
                                // 因为 'a 不能逃逸出 async 块。
                                // 这是一个 "Driver -> Heap" 的拷贝。
                                let data_vec = frame.data.to_vec();
                                let w = frame.width;
                                let h = frame.height;

                                // 发送回主线程
                                let _ = res_tx.send(Response::FrameData {
                                    width: w,
                                    height: h,
                                    data: data_vec,
                                });
                            }
                            Err(e) => {
                                // 发送错误信息
                                let _ = res_tx.send(Response::Error(e.to_string()));
                            }
                        }
                    }
                    Command::Stop => break,
                }
            }
            // 任务结束，自动 Stop Stream
            let _ = stream.stop().await;
        });

        Ok(Self {
            cmd_tx,
            res_rx,
            width: 0, // 初始化为0，读取第一帧后更新
            height: 0,
            is_opened: true,
        })
    }

    /// 核心 API：读取下一帧
    ///
    /// # 返回值
    /// * `Ok(true)` - 读取成功
    /// * `Ok(false)` - 流结束
    /// * `Err(e)` - 硬件错误
    pub fn read(&mut self, mat: &mut Mat) -> Result<bool> {
        if !self.is_opened {
            return Ok(false);
        }

        // 1. 发送“抓取”指令
        if self.cmd_tx.send(Command::NextFrame).is_err() {
            return Err(anyhow!("Background worker is dead"));
        }

        // 2. 阻塞等待结果 (同步 API 的本质)
        let response = self
            .res_rx
            .recv()
            .map_err(|_| anyhow!("Failed to receive response from worker"))?;

        match response {
            Response::FrameData {
                width,
                height,
                data,
            } => {
                // 更新内部属性
                self.width = width as i32;
                self.height = height as i32;

                // 【Buffer Swapping / Zero-Copy 优化逻辑】
                // 如果 mat 的尺寸和新帧完全一致，我们直接交换内部指针 (swap data)
                // 这样避免了在主线程进行第二次内存拷贝。

                // 检查用户传进来的 Mat 是否需要重新分配
                // let required_size = data.len();

                // 这里我们做了一个简单的 "Move" 操作
                // 我们直接把从后台线程拿到的 Vec<u8> 赋值给 Mat
                // 旧的 Mat 数据会被释放。
                mat.data = data;
                mat.rows = height as i32;
                mat.cols = width as i32;
                mat.channels = 3; // 假设 BGR/RGB，具体需根据 FourCC 判断，暂简化为 3

                // 计算 step (假设是 Packed)
                mat.step = (mat.cols * mat.channels as i32) as usize;

                Ok(true)
            }
            Response::Error(msg) => Err(anyhow!("Capture error: {}", msg)),
            Response::EndOfStream => Ok(false),
        }
    }

    /// 检查摄像头是否打开
    pub fn is_opened(&self) -> bool {
        self.is_opened
    }

    /// 获取属性 (强类型)
    pub fn get_width(&self) -> i32 {
        self.width
    }

    pub fn get_height(&self) -> i32 {
        self.height
    }
}

// 析构函数：通知后台线程退出
impl Drop for VideoCapture {
    fn drop(&mut self) {
        // 尝试发送停止信号，忽略错误（因为 worker 可能已经退出了）
        let _ = self.cmd_tx.send(Command::Stop);
    }
}
