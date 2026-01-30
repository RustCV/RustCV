use anyhow::{anyhow, Result};
use rustcv_core::builder::CameraConfig;
use rustcv_core::traits::{Driver, Stream};

/// 后端枚举，用于内部标记当前使用的是哪个驱动
#[derive(Debug, Clone, Copy)]
pub enum BackendType {
    V4L2,
    AVFoundation,
    Dummy, // 用于不支持的系统或测试
}

/// 创建驱动实例的工厂函数
pub fn create_driver() -> Result<Box<dyn Driver>> {
    #[cfg(all(feature = "linux-v4l2", target_os = "linux"))]
    {
        return Ok(Box::new(rustcv_backend_v4l2::V4l2Driver::new()));
    }

    #[cfg(all(feature = "macos-avf", target_os = "macos"))]
    {
        // 注意：这里假设你之前的 AVF Driver 结构体名为 AvfDriver
        return Ok(Box::new(rustcv_backend_avf::AvfDriver::new()));
    }

    // 如果没有匹配的后端，返回错误
    Err(anyhow!(
        "No supported backend found for this OS. Please check Cargo features."
    ))
}

/// 辅助：获取首选后端类型
pub fn default_backend() -> BackendType {
    #[cfg(target_os = "linux")]
    return BackendType::V4L2;
    #[cfg(target_os = "macos")]
    return BackendType::AVFoundation;

    BackendType::Dummy
}
