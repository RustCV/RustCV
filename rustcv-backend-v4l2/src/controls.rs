use std::sync::Arc;
use v4l::control::{Control, Value};
use v4l::Device;
// use v4l::v4l2; // 不再依赖不稳定的自动生成绑定

use rustcv_core::error::{CameraError, Result};
use rustcv_core::traits::{
    DeviceControls, LensControl, SensorControl, SystemControl, TriggerConfig, TriggerMode,
};

// --- 手动定义 V4L2 标准常量 (Linux ABI) ---
// 来源: /usr/include/linux/v4l2-controls.h
// 这样做可以避免 v4l-sys 绑定生成失败导致的 "constant not found" 错误

const V4L2_CID_BASE: u32 = 0x00980000;
const V4L2_CID_CAMERA_CLASS_BASE: u32 = 0x009A0000;

// Gain (增益) 是 User Class Control
const CID_GAIN: u32 = V4L2_CID_BASE + 19; // 0x00980913

// 以下是 Camera Class Controls
const CID_EXPOSURE_AUTO: u32 = V4L2_CID_CAMERA_CLASS_BASE + 1; // 0x009A0901
const CID_EXPOSURE_ABSOLUTE: u32 = V4L2_CID_CAMERA_CLASS_BASE + 2; // 0x009A0902
const CID_FOCUS_AUTO: u32 = V4L2_CID_CAMERA_CLASS_BASE + 10; // 0x009A090A
const CID_FOCUS_ABSOLUTE: u32 = V4L2_CID_CAMERA_CLASS_BASE + 11; // 0x009A090B
const CID_ZOOM_ABSOLUTE: u32 = V4L2_CID_CAMERA_CLASS_BASE + 13; // 0x009A090D

// --- 工厂函数 ---

pub fn create_controls(dev: Arc<Device>) -> DeviceControls {
    DeviceControls {
        sensor: Box::new(V4l2Sensor { dev: dev.clone() }),
        lens: Box::new(V4l2Lens { dev: dev.clone() }),
        system: Box::new(V4l2System { dev }),
    }
}

// --- 1. 传感器控制 (Sensor) ---
struct V4l2Sensor {
    dev: Arc<Device>,
}

impl SensorControl for V4l2Sensor {
    fn set_exposure(&self, value_us: u32) -> Result<()> {
        // 先尝试关闭自动曝光
        let _ = self.dev.set_control(Control {
            id: CID_EXPOSURE_AUTO,
            value: Value::Integer(1), // 1 = V4L2_EXPOSURE_MANUAL
        });

        // 设置绝对曝光值
        self.dev
            .set_control(Control {
                id: CID_EXPOSURE_ABSOLUTE,
                value: Value::Integer(value_us as i64),
            })
            .map_err(CameraError::Io)?;

        Ok(())
    }

    fn get_exposure(&self) -> Result<u32> {
        let val = self
            .dev
            .control(CID_EXPOSURE_ABSOLUTE)
            .map_err(CameraError::Io)?;

        match val.value {
            Value::Integer(v) => Ok(v as u32),
            _ => Err(CameraError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid exposure value type",
            ))),
        }
    }
}

// --- 2. 镜头控制 (Lens) ---
struct V4l2Lens {
    dev: Arc<Device>,
}

impl LensControl for V4l2Lens {
    fn set_zoom(&self, zoom: u32) -> Result<()> {
        self.dev
            .set_control(Control {
                id: CID_ZOOM_ABSOLUTE,
                value: Value::Integer(zoom as i64),
            })
            .map_err(CameraError::Io)
    }

    fn set_focus(&self, focus: u32) -> Result<()> {
        let _ = self.dev.set_control(Control {
            id: CID_FOCUS_AUTO,
            value: Value::Boolean(false),
        });

        self.dev
            .set_control(Control {
                id: CID_FOCUS_ABSOLUTE,
                value: Value::Integer(focus as i64),
            })
            .map_err(CameraError::Io)
    }
}

// --- 3. 系统控制 (System) ---
struct V4l2System {
    dev: Arc<Device>,
}

impl SystemControl for V4l2System {
    unsafe fn force_reset(&self) -> Result<()> {
        Ok(())
    }

    fn set_trigger(&self, config: TriggerConfig) -> Result<()> {
        if config.mode == TriggerMode::Off {
            return Ok(());
        }
        Err(CameraError::FormatNotSupported)
    }

    fn export_state(&self) -> Result<serde_json::Value> {
        use serde_json::json;

        // 使用我们定义的常量
        let exp = self.dev.control(CID_EXPOSURE_ABSOLUTE).ok();
        let gain = self.dev.control(CID_GAIN).ok();

        Ok(json!({
            "backend": "v4l2",
            // 这里的 Value 实现了 Debug，可以直接 format!
            "exposure": exp.map(|v| format!("{:?}", v.value)),
            "gain": gain.map(|v| format!("{:?}", v.value)),
        }))
    }
}
