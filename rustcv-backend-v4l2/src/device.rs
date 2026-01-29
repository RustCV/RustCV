use std::sync::Arc;
use v4l::capability::Flags;
use v4l::video::Capture;

use rustcv_core::builder::CameraConfig;
use rustcv_core::error::{CameraError, Result};
use rustcv_core::pixel_format::PixelFormat;
use rustcv_core::traits::{DeviceControls, DeviceInfo, Stream};

use crate::controls::create_controls;
use crate::pixel_map;
use crate::stream::V4l2Stream; // 将在 Part 3 实现

/// 枚举系统中的摄像头设备
pub fn list_devices() -> Result<Vec<DeviceInfo>> {
    let mut devices = Vec::new();

    // 遍历 /dev/video* 节点
    let node_iter = v4l::context::enum_devices();

    for node in node_iter {
        // 尝试打开设备查询能力
        let path = node.path().to_string_lossy().to_string();
        if let Ok(dev) = v4l::Device::with_path(&path) {
            if let Ok(caps) = dev.query_caps() {
                // 过滤：必须支持 Video Capture (0x00000001)
                // 忽略 Metadata 设备或 Output 设备
                if caps.capabilities.contains(Flags::VIDEO_CAPTURE) {
                    devices.push(DeviceInfo {
                        name: node.name().unwrap_or_else(|| "Unknown Camera".into()),
                        id: path, // /dev/video0
                        backend: "V4L2".to_string(),
                        bus_info: Some(caps.bus),
                    });
                }
            }
        }
    }

    Ok(devices)
}

/// 打开设备并初始化流
pub fn open(id: &str, config: CameraConfig) -> Result<(Box<dyn Stream>, DeviceControls)> {
    // 1. 打开设备句柄
    let dev = v4l::Device::with_path(id).map_err(CameraError::Io)?;

    // 2. 获取格式并进行协商 (Format Negotiation)
    let negotiated_fmt = negotiate_format(&dev, &config)?;

    // 3. 应用格式设置 (ioctl: VIDIOC_S_FMT)
    let mut fmt = dev.format().map_err(CameraError::Io)?;
    fmt.width = negotiated_fmt.width;
    fmt.height = negotiated_fmt.height;
    fmt.fourcc =
        pixel_map::to_v4l_fourcc(negotiated_fmt.format).ok_or(CameraError::FormatNotSupported)?;
    // 注意：FPS 设置通常需要 VIDIOC_S_PARM，这里简化处理，稍后在 Stream 初始化中设置

    let applied_fmt = dev.set_format(&fmt).map_err(CameraError::Io)?;

    tracing::info!(
        "Camera opened: {}x{} @ {}",
        applied_fmt.width,
        applied_fmt.height,
        applied_fmt.fourcc
    );

    // 4. 创建共享句柄 (Arc)
    // Stream 和 Controls 都需要访问同一个 fd，但在 V4L2 中多线程访问同一个 fd 是安全的
    let dev_arc = Arc::new(dev);

    // 5. 初始化流 (申请 Buffer, mmap)
    let stream = V4l2Stream::new(dev_arc.clone(), &applied_fmt, config.buffer_count)?;

    // 6. 初始化控制器 (Sensor, Lens, System)
    let controls = create_controls(dev_arc);

    Ok((Box::new(stream), controls))
}

/// 核心：格式协商算法
/// 遍历硬件支持的所有格式，计算得分，返回最佳配置
struct NegotiatedFormat {
    width: u32,
    height: u32,
    format: PixelFormat,
    #[allow(dead_code)]
    fps: u32, // 目标 FPS
}

fn negotiate_format(dev: &v4l::Device, config: &CameraConfig) -> Result<NegotiatedFormat> {
    let mut best_score = -1;
    let mut best_fmt = None;

    // 获取设备支持的所有格式
    let supported_formats = dev.enum_formats().map_err(CameraError::Io)?;

    for v4l_fmt in supported_formats {
        let core_fmt = pixel_map::from_v4l_fourcc(v4l_fmt.fourcc);

        // 获取该格式下的所有分辨率
        let resolutions = dev.enum_framesizes(v4l_fmt.fourcc).unwrap_or_default();

        for res in resolutions {
            // 这里简化处理 Discrete 分辨率，Stepwise 暂略
            for size in res.size.to_discrete() {
                // 计算得分
                let current_score = calculate_score(config, size.width, size.height, core_fmt);

                if current_score > best_score {
                    best_score = current_score;
                    best_fmt = Some(NegotiatedFormat {
                        width: size.width,
                        height: size.height,
                        format: core_fmt,
                        fps: 30, // 默认 30，实际上应该进一步 enum_frameintervals
                    });
                }
            }
        }
    }

    best_fmt.ok_or(CameraError::FormatNotSupported)
}

fn calculate_score(config: &CameraConfig, w: u32, h: u32, fmt: PixelFormat) -> i32 {
    let mut score = 0;

    // 1. 匹配分辨率
    for (req_w, req_h, prio) in &config.resolution_req {
        if w == *req_w && h == *req_h {
            score += *prio as i32 * 10;
        }
    }

    // 2. 匹配格式
    for (req_fmt, prio) in &config.format_req {
        if fmt == *req_fmt {
            score += *prio as i32 * 10;
        }
    }

    // 3. 分辨率越大基础分越高 (作为 Tie-breaker)
    score += (w / 100) as i32;

    score
}
