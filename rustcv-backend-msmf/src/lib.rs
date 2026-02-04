#![cfg(target_os = "windows")]

pub mod controls;
pub mod device;
pub mod pixel_map;
pub mod stream;

use rustcv_core::error::Result;
use rustcv_core::traits::{DeviceControls, DeviceInfo, Driver, Stream};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct MsmfDriver;

impl Default for MsmfDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl MsmfDriver {
    pub fn new() -> Self {
        Self
    }
}

impl Driver for MsmfDriver {
    fn list_devices(&self) -> Result<Vec<DeviceInfo>> {
        device::list_devices()
    }

    fn open(
        &self,
        id: &str,
        config: rustcv_core::builder::CameraConfig,
    ) -> Result<(Box<dyn Stream>, DeviceControls)> {
        device::open(id, config)
    }
}


pub fn default_driver() -> Arc<dyn Driver> {
    Arc::new(MsmfDriver::new())
}
