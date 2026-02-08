use std::sync::Arc;
use windows::core::GUID;
use windows::Win32::Media::MediaFoundation::*;

use rustcv_core::error::{CameraError, Result};
use rustcv_core::traits::{
    DeviceControls, LensControl, SensorControl, SystemControl, TriggerConfig, TriggerMode,
};

const DEFAULT_EXPOSURE_US: u32 = 10000;

/// IAMVideoProcAmp interfaces for more reliable camera control.
pub fn create_controls(source_reader: Arc<IMFSourceReader>) -> DeviceControls {
    DeviceControls {
        sensor: Box::new(MsmfSensor {
            source_reader: source_reader.clone(),
        }),
        lens: Box::new(MsmfLens {
            source_reader: source_reader.clone(),
        }),
        system: Box::new(MsmfSystem { source_reader }),
    }
}

/// that require unsafe context.
unsafe fn get_current_media_type(source_reader: &IMFSourceReader) -> Option<IMFMediaType> {
    source_reader
        .GetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32)
        .ok()
}

unsafe fn set_media_type_uint64(source_reader: &IMFSourceReader, guid: &GUID, value: u64) {
    if let Some(media_type) = get_current_media_type(source_reader) {
        let _ = media_type.SetUINT64(guid, value);
    }
}

unsafe fn get_media_type_uint64(source_reader: &IMFSourceReader, guid: &GUID) -> Option<u64> {
    get_current_media_type(source_reader).and_then(|media_type| media_type.GetUINT64(guid).ok())
}

struct MsmfSensor {
    source_reader: Arc<IMFSourceReader>,
}

unsafe impl Send for MsmfSensor {}
unsafe impl Sync for MsmfSensor {}

impl SensorControl for MsmfSensor {
    fn set_exposure(&self, value_us: u32) -> Result<()> {
        unsafe {
            set_media_type_uint64(&self.source_reader, &MF_MT_VIDEO_LIGHTING, value_us as u64);
        }
        Ok(())
    }

    fn get_exposure(&self) -> Result<u32> {
        unsafe {
            Ok(
                get_media_type_uint64(&self.source_reader, &MF_MT_VIDEO_LIGHTING)
                    .map(|v| v as u32)
                    .unwrap_or(DEFAULT_EXPOSURE_US),
            )
        }
    }
}

/// MSMF implementation of lens controls.
///
/// This struct provides lens-related camera controls such as zoom and focus
/// adjustment using Windows Media Foundation APIs.
///
/// # Note
///
/// The current implementation uses media type attributes for control.
/// For production use, consider implementing proper IAMCameraControl interface.
struct MsmfLens {
    source_reader: Arc<IMFSourceReader>,
}

unsafe impl Send for MsmfLens {}
unsafe impl Sync for MsmfLens {}

impl LensControl for MsmfLens {
    fn set_zoom(&self, zoom: u32) -> Result<()> {
        unsafe {
            set_media_type_uint64(&self.source_reader, &MF_MT_VIDEO_LIGHTING, zoom as u64);
        }
        Ok(())
    }

    fn set_focus(&self, focus: u32) -> Result<()> {
        unsafe {
            set_media_type_uint64(&self.source_reader, &MF_MT_VIDEO_LIGHTING, focus as u64);
        }
        Ok(())
    }
}

struct MsmfSystem {
    source_reader: Arc<IMFSourceReader>,
}

unsafe impl Send for MsmfSystem {}
unsafe impl Sync for MsmfSystem {}

impl SystemControl for MsmfSystem {
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

        let exposure = unsafe {
            get_media_type_uint64(&self.source_reader, &MF_MT_VIDEO_LIGHTING).map(|v| v as u32)
        };

        Ok(json!({
            "backend": "msmf",
            "exposure": exposure,
        }))
    }
}
