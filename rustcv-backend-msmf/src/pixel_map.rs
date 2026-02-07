//! Pixel format mapping between Media Foundation and RustCV.
//!
//! This module provides bidirectional conversion between Windows Media Foundation
//! pixel format GUIDs and RustCV's `PixelFormat` enum.
//!
//! # Supported Formats
//!
//! | MF GUID | FourCC | Description |
//! |---------|--------|-------------|
//! | `MFVideoFormat_YUY2` | YUYV | YUV 4:2:2 packed |
//! | `MFVideoFormat_UYVY` | UYVY | YUV 4:2:2 packed (byte-swapped) |
//! | `MFVideoFormat_NV12` | NV12 | YUV 4:2:0 semi-planar |
//! | `MFVideoFormat_YV12` | YV12 | YUV 4:2:0 planar |
//! | `MFVideoFormat_RGB24` | RGB3 | 24-bit RGB |
//! | `MFVideoFormat_RGB32` | RGBA | 32-bit RGBA |
//! | `MFVideoFormat_MJPG` | MJPEG | Motion JPEG |
//! | `MFVideoFormat_H264` | H264 | H.264/AVC |

#![allow(non_upper_case_globals)]

use rustcv_core::pixel_format::{FourCC, PixelFormat};
use windows::core::GUID;
use windows::Win32::Media::MediaFoundation::*;

/// Converts a Media Foundation GUID to a RustCV pixel format.
///
/// This function maps Windows Media Foundation video format GUIDs
/// to the cross-platform `PixelFormat` enum used by RustCV.
///
/// # Arguments
///
/// * `guid` - The Media Foundation format GUID
///
/// # Returns
///
/// The corresponding `PixelFormat`. Returns `PixelFormat::Unknown(0)`
/// if the format is not recognized, with a warning logged.
pub fn from_mf_guid(guid: &GUID) -> PixelFormat {
    match *guid {
        MFVideoFormat_YUY2 => PixelFormat::Known(FourCC::YUYV),
        MFVideoFormat_UYVY => PixelFormat::Known(FourCC::UYVY),
        MFVideoFormat_NV12 => PixelFormat::Known(FourCC::NV12),
        MFVideoFormat_YV12 => PixelFormat::Known(FourCC::YV12),
        MFVideoFormat_RGB24 => PixelFormat::Known(FourCC::RGB3),
        MFVideoFormat_RGB32 => PixelFormat::Known(FourCC::RGBA),
        MFVideoFormat_MJPG => PixelFormat::Known(FourCC::MJPEG),
        MFVideoFormat_H264 => PixelFormat::Known(FourCC::H264),
        _ => {
            tracing::warn!(target: "rustcv::msmf", "Unknown MF pixel format: {:?}", guid);
            PixelFormat::Unknown(0)
        }
    }
}

/// Converts a RustCV pixel format to a Media Foundation GUID.
///
/// This function provides the reverse mapping from `PixelFormat`
/// to Windows Media Foundation format GUIDs.
///
/// # Arguments
///
/// * `fmt` - The RustCV pixel format
///
/// # Returns
///
/// `Some(GUID)` if the format is supported, `None` otherwise.
pub fn to_mf_guid(fmt: PixelFormat) -> Option<GUID> {
    match fmt {
        PixelFormat::Known(FourCC::YUYV) => Some(MFVideoFormat_YUY2),
        PixelFormat::Known(FourCC::UYVY) => Some(MFVideoFormat_UYVY),
        PixelFormat::Known(FourCC::NV12) => Some(MFVideoFormat_NV12),
        PixelFormat::Known(FourCC::YV12) => Some(MFVideoFormat_YV12),
        PixelFormat::Known(FourCC::RGB3) => Some(MFVideoFormat_RGB24),
        PixelFormat::Known(FourCC::RGBA) => Some(MFVideoFormat_RGB32),
        PixelFormat::Known(FourCC::MJPEG) => Some(MFVideoFormat_MJPG),
        PixelFormat::Known(FourCC::H264) => Some(MFVideoFormat_H264),
        _ => None,
    }
}
