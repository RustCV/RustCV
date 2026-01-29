use rustcv_core::pixel_format::{FourCC, PixelFormat};
use v4l::format::fourcc::FourCC as V4lFourCC;

/// 将 v4l crate 的 FourCC 转换为 rustcv-core 的 FourCC
pub fn from_v4l_fourcc(cc: V4lFourCC) -> PixelFormat {
    // 提取 u32 原始值
    let code: u32 = cc.into();
    let core_cc = FourCC(code);

    match core_cc {
        // --- 1. 标准 YUV ---
        FourCC::YUYV => PixelFormat::Known(FourCC::YUYV),
        FourCC::UYVY => PixelFormat::Known(FourCC::UYVY),
        FourCC::NV12 => PixelFormat::Known(FourCC::NV12),
        FourCC::YV12 => PixelFormat::Known(FourCC::YV12),

        // --- 2. 标准 RGB ---
        FourCC::BGR3 => PixelFormat::Known(FourCC::BGR3),
        FourCC::RGB3 => PixelFormat::Known(FourCC::RGB3),

        // --- 3. 压缩格式 ---
        FourCC::MJPEG => PixelFormat::Known(FourCC::MJPEG),
        FourCC::H264 => PixelFormat::Known(FourCC::H264),

        // --- 4. Bayer 格式 (这部分最容易出问题，不同内核版本定义可能不同) ---
        // V4L2 定义：BA81 (BGGR), GB RG, etc.
        FourCC::BA81 => PixelFormat::Known(FourCC::BA81),
        FourCC::GBRG => PixelFormat::Known(FourCC::GBRG),
        FourCC::GRBG => PixelFormat::Known(FourCC::GRBG),
        FourCC::RGGB => PixelFormat::Known(FourCC::RGGB),

        // --- 5. 未知/私有格式 ---
        _ => {
            tracing::warn!(target: "rustcv::v4l2", "Unknown V4L2 pixel format: {}", core_cc.to_string());
            PixelFormat::Unknown(code)
        }
    }
}

/// 将 rustcv-core 的 FourCC 转换为 v4l 的 FourCC
/// 用于请求设备设置格式
pub fn to_v4l_fourcc(fmt: PixelFormat) -> Option<V4lFourCC> {
    match fmt {
        PixelFormat::Known(cc) => Some(V4lFourCC::new(&cc.0.to_le_bytes())),
        PixelFormat::Unknown(_) => None, // 无法主动请求未知的格式
    }
}
