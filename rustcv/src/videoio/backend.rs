use anyhow::Result;
use rustcv_core::traits::Driver;

/// 后端枚举，用于内部标记当前使用的是哪个驱动
#[derive(Debug, Clone, Copy)]
pub enum BackendType {
    V4L2,
    AVFoundation,
    Dummy, // 用于不支持的系统或测试
}

/// 创建驱动实例的工厂函数
pub fn create_driver() -> Result<Box<dyn Driver>> {
    // 根据当前操作系统，直接选用原生后端 (不再需要 feature 门控)
    #[cfg(target_os = "linux")]
    {
        Ok(Box::new(rustcv_backend_v4l2::V4l2Driver::new()))
    }

    #[cfg(target_os = "macos")]
    {
        Ok(Box::new(rustcv_backend_avf::AvfDriver::new()))
    }

    #[cfg(target_os = "windows")]
    {
        Ok(Box::new(rustcv_backend_msmf::MsmfDriver::new()))
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        // 如果没有匹配的后端，返回错误
        Err(anyhow!(
            "No supported backend found for this OS. Please check Cargo features."
        ))
    }
}

/// 辅助：获取首选后端类型
pub fn default_backend() -> BackendType {
    if cfg!(target_os = "linux") {
        BackendType::V4L2
    } else if cfg!(target_os = "macos") {
        BackendType::AVFoundation
    } else {
        BackendType::Dummy
    }
}
