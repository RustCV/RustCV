use windows::core::GUID;
use windows::Win32::Media::MediaFoundation::*;

use rustcv_core::error::{CameraError, Result};
use rustcv_core::traits::{
    DeviceControls, LensControl, SensorControl, SystemControl, TriggerConfig, TriggerMode,
};

use crate::stream::SendableSourceReader;

/// Default exposure time in microseconds (10ms).
const DEFAULT_EXPOSURE_US: u32 = 10000;

pub fn create_controls(source_reader: SendableSourceReader) -> DeviceControls {
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

/// Gets the current media type from the source reader.
unsafe fn get_current_media_type(source_reader: &IMFSourceReader) -> Option<IMFMediaType> {
    source_reader
        .GetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32)
        .ok()
}

/// Sets a 64-bit unsigned integer attribute on the current media type.
unsafe fn set_media_type_uint64(source_reader: &IMFSourceReader, guid: &GUID, value: u64) {
    if let Some(media_type) = get_current_media_type(source_reader) {
        let _ = media_type.SetUINT64(guid, value);
    }
}

/// Gets a 64-bit unsigned integer attribute from the current media type.
unsafe fn get_media_type_uint64(source_reader: &IMFSourceReader, guid: &GUID) -> Option<u64> {
    get_current_media_type(source_reader).and_then(|media_type| media_type.GetUINT64(guid).ok())
}

/// Sensor control implementation for MSMF cameras.
///
/// Provides exposure control through the Media Foundation API.
struct MsmfSensor {
    source_reader: SendableSourceReader,
}

unsafe impl Send for MsmfSensor {}
unsafe impl Sync for MsmfSensor {}

impl SensorControl for MsmfSensor {
    /// Sets the camera exposure time.
    ///
    /// # Arguments
    ///
    /// * `value_us` - Exposure time in microseconds
    fn set_exposure(&self, value_us: u32) -> Result<()> {
        unsafe {
            set_media_type_uint64(
                &self.source_reader.0,
                &MF_MT_VIDEO_LIGHTING,
                value_us as u64,
            );
        }
        Ok(())
    }

    /// Gets the current exposure time.
    ///
    /// # Returns
    ///
    /// The exposure time in microseconds, or a default value if unavailable.
    fn get_exposure(&self) -> Result<u32> {
        unsafe {
            Ok(
                get_media_type_uint64(&self.source_reader.0, &MF_MT_VIDEO_LIGHTING)
                    .map(|v| v as u32)
                    .unwrap_or(DEFAULT_EXPOSURE_US),
            )
        }
    }
}

/// Lens control implementation for MSMF cameras.
///
/// Provides focus and zoom control through the Media Foundation API.
struct MsmfLens {
    source_reader: SendableSourceReader,
}

unsafe impl Send for MsmfLens {}
unsafe impl Sync for MsmfLens {}

impl LensControl for MsmfLens {
    fn set_zoom(&self, zoom: u32) -> Result<()> {
        unsafe {
            set_media_type_uint64(&self.source_reader.0, &MF_MT_VIDEO_LIGHTING, zoom as u64);
        }
        Ok(())
    }

    /// Sets the camera focus position.
    ///
    /// # Arguments
    ///
    /// * `focus` - Focus position (device-specific range)
    fn set_focus(&self, focus: u32) -> Result<()> {
        unsafe {
            set_media_type_uint64(&self.source_reader.0, &MF_MT_VIDEO_LIGHTING, focus as u64);
        }
        Ok(())
    }
}

/// System control implementation for MSMF cameras.
///
/// Provides system-level operations such as reset and trigger control.
struct MsmfSystem {
    source_reader: SendableSourceReader,
}

unsafe impl Send for MsmfSystem {}
unsafe impl Sync for MsmfSystem {}

impl SystemControl for MsmfSystem {
    unsafe fn force_reset(&self) -> Result<()> {
        Ok(())
    }

    /// Configures the camera trigger mode.
    ///
    /// # Arguments
    ///
    /// * `config` - Trigger configuration
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if trigger is disabled, or an error for other modes
    /// as hardware trigger is not supported.
    fn set_trigger(&self, config: TriggerConfig) -> Result<()> {
        if config.mode == TriggerMode::Off {
            return Ok(());
        }
        Err(CameraError::FormatNotSupported)
    }

    fn export_state(&self) -> Result<serde_json::Value> {
        use serde_json::json;

        let exposure = unsafe {
            get_media_type_uint64(&self.source_reader.0, &MF_MT_VIDEO_LIGHTING).map(|v| v as u32)
        };

        Ok(json!({
            "backend": "msmf",
            "exposure": exposure,
        }))
    }
}
