#![allow(non_upper_case_globals)]

use rustcv_core::pixel_format::{FourCC, PixelFormat};
use windows::core::GUID;
use windows::Win32::Media::MediaFoundation::*;

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
