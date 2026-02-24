//! # RustCV MSMF Backend
//!
//! Windows Media Foundation (MSMF) backend for the RustCV camera framework.
//!
//! ## Overview
//!
//! This crate provides a Windows-specific implementation of the RustCV camera
//! interface using the Media Foundation API. It enables video capture from
//! USB cameras and other video capture devices on Windows systems.
//!
//! ## Features
//!
//! - **Device Enumeration**: List all connected video capture devices
//! - **Format Negotiation**: Automatic selection of optimal video format
//! - **Async Streaming**: Non-blocking frame capture using tokio
//! - **Multiple Pixel Formats**: Support for YUYV, NV12, UYVY, and more
//! - **Camera Controls**: Exposure, focus, zoom, and other device controls
//!
//! ## Supported Pixel Formats
//!
//! | Format | Description | Layout |
//! |--------|-------------|--------|
//! | YUYV (YUY2) | YUV 4:2:2 packed | Packed |
//! | UYVY | YUV 4:2:2 packed (byte-swapped) | Packed |
//! | NV12 | YUV 4:2:0 semi-planar | Planar |
//! | YV12 | YUV 4:2:0 planar | Planar |
//! | RGB24 | 24-bit RGB | Packed |
//! | RGB32 | 32-bit RGBA | Packed |
//! | MJPEG | Motion JPEG | Compressed |
//! | H264 | H.264/AVC | Compressed |
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                      Application Layer                       │
//! └─────────────────────────────────────────────────────────────┘
//!                               │
//!                               ▼
//! ┌─────────────────────────────────────────────────────────────┐
//! │  MsmfDriver (implements Driver trait)                        │
//! │  ├── list_devices() → Device enumeration                     │
//! │  └── open() → Creates MsmfStream + DeviceControls            │
//! └─────────────────────────────────────────────────────────────┘
//!                               │
//!                               ▼
//! ┌─────────────────────────────────────────────────────────────┐
//! │  MsmfStream (implements Stream trait)                        │
//! │  ├── start() / stop() → Stream lifecycle                     │
//! │  ├── next_frame() → Async frame capture                      │
//! │  └── Buffer management with stride handling                  │
//! └─────────────────────────────────────────────────────────────┘
//!                               │
//!                               ▼
//! ┌─────────────────────────────────────────────────────────────┐
//! │  Windows Media Foundation API                                │
//! │  ├── IMFMediaSource → Device representation                  │
//! │  ├── IMFSourceReader → Async frame reader                    │
//! │  └── IMFSourceReaderCallback → Frame callback                │
//! └─────────────────────────────────────────────────────────────┘
//! ```

#![cfg(target_os = "windows")]

pub mod controls;
pub mod device;
pub mod pixel_map;
pub mod stream;

use rustcv_core::error::Result;
use rustcv_core::traits::{DeviceControls, DeviceInfo, Driver, Stream};
use std::sync::Arc;

/// Media Foundation driver for Windows camera devices.
///
/// This is the main entry point for the MSMF backend. It implements the
/// [`Driver`] trait and provides device enumeration and stream creation.
///
/// # Example
///
/// ```rust,no_run
/// use rustcv_backend_msmf::MsmfDriver;
/// use rustcv_core::traits::Driver;
///
/// let driver = MsmfDriver::new();
/// let devices = driver.list_devices()?;
/// println!("Found {} cameras", devices.len());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone)]
pub struct MsmfDriver;

impl Default for MsmfDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl MsmfDriver {
    /// Creates a new MSMF driver instance.
    ///
    /// This constructor is lightweight and does not initialize Media Foundation.
    /// Initialization is deferred until the first operation that requires it.
    pub fn new() -> Self {
        Self
    }
}

impl Driver for MsmfDriver {
    /// Lists all available video capture devices.
    ///
    /// This method initializes Media Foundation if needed and enumerates
    /// all connected video capture devices.
    ///
    /// # Returns
    ///
    /// A vector of [`DeviceInfo`] containing device name, ID, and backend info.
    /// Returns an empty vector if no devices are found.
    fn list_devices(&self) -> Result<Vec<DeviceInfo>> {
        device::list_devices()
    }

    /// Opens a camera device for video capture.
    ///
    /// This method:
    /// 1. Initializes Media Foundation if needed
    /// 2. Creates a media source for the specified device
    /// 3. Negotiates the best format based on configuration
    /// 4. Creates an async stream for frame capture
    ///
    /// # Arguments
    ///
    /// * `id` - Device identifier (symbolic link from device enumeration)
    /// * `config` - Camera configuration with resolution, FPS, and format preferences
    ///
    /// # Returns
    ///
    /// A tuple containing:
    /// - `Box<dyn Stream>` - The camera stream for frame capture
    /// - `DeviceControls` - Control interfaces for camera settings
    fn open(
        &self,
        id: &str,
        config: rustcv_core::builder::CameraConfig,
    ) -> Result<(Box<dyn Stream>, DeviceControls)> {
        device::open(id, config)
    }
}

/// Returns the default MSMF driver as a trait object.
///
/// This is a convenience function for applications that need a
/// dynamically dispatched driver instance.
pub fn default_driver() -> Arc<dyn Driver> {
    Arc::new(MsmfDriver::new())
}
