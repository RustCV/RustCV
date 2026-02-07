#![cfg(target_os = "macos")]

mod delegate;
mod gcd;
mod stream;

use anyhow::Result;
use objc2_av_foundation::{
    AVCaptureDeviceDiscoverySession,
    // 1. 整数枚举 (Enum) -> 引入类型名
    AVCaptureDevicePosition,
    // 2. 字符串常量 (String Constants) -> 直接引入完整的长常量名
    AVCaptureDeviceTypeBuiltInWideAngleCamera,
    AVCaptureDeviceTypeExternal, // Replaced Unknown with External
    AVMediaTypeVideo,
};
use objc2_foundation::NSArray;
use rustcv_core::builder::CameraConfig;
use rustcv_core::error::CameraError;
use rustcv_core::traits::{DeviceControls, DeviceInfo, Driver, Stream};

pub struct AvfDriver;

impl Default for AvfDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl AvfDriver {
    pub fn new() -> Self {
        Self
    }
}

impl Driver for AvfDriver {
    fn list_devices(&self) -> Result<Vec<DeviceInfo>, CameraError> {
        let mut result = Vec::new();
        unsafe {
            // AVCaptureDeviceTypeExternalUnknown is deprecated, use AVCaptureDeviceTypeExternal
            let types = NSArray::from_slice(&[
                AVCaptureDeviceTypeBuiltInWideAngleCamera,
                AVCaptureDeviceTypeExternal,
            ]);

            // 【修正 1】直接获取 session，不需要 Option 解包
            // 这个方法返回 Retained<AVCaptureDeviceDiscoverySession>，永远不为 nil
            let session =
                AVCaptureDeviceDiscoverySession::discoverySessionWithDeviceTypes_mediaType_position(
                    &types,
                    AVMediaTypeVideo,
                    AVCaptureDevicePosition::Unspecified,
                );

            // 【修正 2】直接遍历
            // session.devices() 返回 Retained<NSArray<AVCaptureDevice>>
            // 这是一个实现了 IntoIterator 的类型
            for dev in session.devices() {
                result.push(DeviceInfo {
                    name: dev.localizedName().to_string(),
                    id: dev.uniqueID().to_string(),
                    backend: "AVFoundation".to_string(),
                    bus_info: None,
                });
            }
        }
        Ok(result)
    }

    fn open(
        &self,
        id: &str,
        _config: CameraConfig,
    ) -> Result<(Box<dyn Stream>, DeviceControls), CameraError> {
        let stream = stream::AvfStream::new(id)
            .map_err(|e| CameraError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        let controls = create_dummy_controls();
        Ok((Box::new(stream), controls))
    }
}

fn create_dummy_controls() -> DeviceControls {
    DeviceControls {
        sensor: Box::new(DummyControl),
        lens: Box::new(DummyControl),
        system: Box::new(DummyControl),
    }
}

struct DummyControl;

use rustcv_core::traits::{LensControl, SensorControl, SystemControl, TriggerConfig};

impl SensorControl for DummyControl {
    fn set_exposure(&self, _value_us: u32) -> Result<(), CameraError> {
        Ok(())
    }
    fn get_exposure(&self) -> Result<u32, CameraError> {
        Ok(0)
    }
}

impl LensControl for DummyControl {
    fn set_zoom(&self, _zoom: u32) -> Result<(), CameraError> {
        Ok(())
    }
    fn set_focus(&self, _focus: u32) -> Result<(), CameraError> {
        Ok(())
    }
}

impl SystemControl for DummyControl {
    unsafe fn force_reset(&self) -> Result<(), CameraError> {
        Ok(())
    }
    fn set_trigger(&self, _config: TriggerConfig) -> Result<(), CameraError> {
        Ok(())
    }
    fn export_state(&self) -> Result<serde_json::Value, CameraError> {
        Ok(serde_json::Value::Null)
    }
}
