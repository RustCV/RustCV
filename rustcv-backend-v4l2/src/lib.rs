pub mod controls;
pub mod device;
pub mod pixel_map;
pub mod stream;

use rustcv_core::error::Result;
use rustcv_core::traits::Driver;
use std::sync::Arc;

/// V4L2 驱动单例结构体
/// 通常作为全局单例存在
#[derive(Debug, Clone)]
pub struct V4l2Driver;

impl Default for V4l2Driver {
    fn default() -> Self {
        Self::new()
    }
}

impl V4l2Driver {
    pub fn new() -> Self {
        Self
    }
}

// 实现 Driver Trait
impl Driver for V4l2Driver {
    fn list_devices(&self) -> Result<Vec<rustcv_core::traits::DeviceInfo>> {
        device::list_devices()
    }

    fn open(
        &self,
        id: &str,
        config: rustcv_core::builder::CameraConfig,
    ) -> Result<(
        Box<dyn rustcv_core::traits::Stream>,
        rustcv_core::traits::DeviceControls,
    )> {
        device::open(id, config)
    }
}

// 为了方便直接使用，提供一个默认实例
pub fn default_driver() -> Arc<dyn Driver> {
    Arc::new(V4l2Driver::new())
}
