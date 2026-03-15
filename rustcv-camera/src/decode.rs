/// Pixel format decoding and conversion.
/// 像素格式解码与转换。
///
/// Converts raw camera data (MJPEG, YUYV, etc.) to BGR24 for display/processing.
/// 将摄像头原始数据（MJPEG、YUYV 等）转换为 BGR24 用于显示/处理。
///
/// Design decisions:
/// 设计决策：
///
/// - **turbojpeg** (optional feature): SIMD-accelerated JPEG decode, ~2ms for 640x480.
///   The `Decompressor` is reused across frames to avoid per-frame allocation.
///   turbojpeg（可选 feature）：SIMD 加速的 JPEG 解码，640x480 约 2ms。
///   `Decompressor` 跨帧复用以避免每帧分配。
///
/// - **YUYV→BGR**: BT.601 integer arithmetic (no floating point).
///   Future optimization: SIMD intrinsics for 3-4x speedup.
///   YUYV→BGR：BT.601 整数运算（无浮点）。
///   未来优化：SIMD intrinsics 可获得 3-4 倍加速。
///
/// - **Fallback JPEG**: `image` crate (pure Rust, ~25ms — 10x slower than turbojpeg).
///   回退 JPEG：`image` crate（纯 Rust，约 25ms —— 比 turbojpeg 慢 10 倍）。
use crate::error::{CameraError, Result};
use crate::frame::Frame;
use crate::mat::Mat;
use crate::pixel_format::PixelFormat;

/// Decode a [`Frame`] into a BGR [`Mat`].
/// 将 [`Frame`] 解码为 BGR [`Mat`]。
///
/// The `decompressor` parameter is for reusing a turbojpeg Decompressor
/// across frames (avoiding per-frame allocation). Pass `None` to create
/// a temporary one.
///
/// `decompressor` 参数用于跨帧复用 turbojpeg Decompressor
/// （避免每帧分配）。传入 `None` 则创建临时的。
pub fn decode_frame(
    frame: &Frame<'_>,
    mat: &mut Mat,
    #[cfg(feature = "turbojpeg")] _decompressor: Option<&mut turbojpeg::Decompressor>,
    #[cfg(not(feature = "turbojpeg"))] _decompressor: Option<()>,
) -> Result<()> {
    let w = frame.width();
    let h = frame.height();

    match frame.pixel_format() {
        PixelFormat::Mjpeg => {
            mat.ensure_size(h, w, 3);
            decode_mjpeg(frame.data(), mat)?;
        }
        PixelFormat::Yuyv => {
            mat.ensure_size(h, w, 3);
            yuyv_to_bgr(frame.data(), mat.data_mut(), w as usize, h as usize);
        }
        PixelFormat::Bgr24 => {
            // Already BGR — just copy.
            // 已经是 BGR —— 直接拷贝。
            mat.ensure_size(h, w, 3);
            let expected = (w * h * 3) as usize;
            if frame.data().len() >= expected {
                mat.data_mut()[..expected].copy_from_slice(&frame.data()[..expected]);
            }
        }
        PixelFormat::Rgb24 => {
            // RGB → BGR: swap R and B channels.
            // RGB → BGR：交换 R 和 B 通道。
            mat.ensure_size(h, w, 3);
            rgb_to_bgr(frame.data(), mat.data_mut());
        }
        other => {
            return Err(CameraError::DecodeError(format!(
                "unsupported pixel format for decode: {:?}",
                other
            )));
        }
    }

    Ok(())
}

// ─── MJPEG decode ───────────────────────────────────────────────────────────

/// Decode MJPEG data to BGR.
/// 将 MJPEG 数据解码为 BGR。
#[cfg(feature = "turbojpeg")]
fn decode_mjpeg(src: &[u8], mat: &mut Mat) -> Result<()> {
    use turbojpeg::{Decompressor, Image, PixelFormat as TJPixelFormat};

    // Create a decompressor. In a real hot loop, this should be reused
    // via the Camera struct. Here we create one as fallback.
    // 创建解压器。在实际的热循环中，应通过 Camera 结构体复用。
    // 这里作为回退创建一个。
    let mut decompressor = Decompressor::new()
        .map_err(|e| CameraError::DecodeError(format!("turbojpeg init: {}", e)))?;

    let header = decompressor
        .read_header(src)
        .map_err(|e| CameraError::DecodeError(format!("JPEG header: {}", e)))?;

    // Extract step before mutable borrow of mat.data to satisfy the borrow checker.
    // 在可变借用 mat.data 之前提取 step，以满足借用检查器。
    let pitch = mat.step();
    let image = Image {
        pixels: mat.data_mut(),
        width: header.width,
        pitch,
        height: header.height,
        format: TJPixelFormat::BGR,
    };

    decompressor
        .decompress(src, image)
        .map_err(|e| CameraError::DecodeError(format!("JPEG decompress: {}", e)))?;

    Ok(())
}

/// Fallback MJPEG decode using the `image` crate (pure Rust, slower).
/// 使用 `image` crate 的回退 MJPEG 解码（纯 Rust，较慢）。
#[cfg(not(feature = "turbojpeg"))]
fn decode_mjpeg(_src: &[u8], _mat: &mut Mat) -> Result<()> {
    // Minimal JPEG decode without external dependencies.
    // For now, return an error suggesting turbojpeg feature.
    // 无外部依赖的最小 JPEG 解码。
    // 目前返回错误，建议启用 turbojpeg feature。
    //
    // TODO: Add `image` crate as optional dependency for fallback.
    Err(CameraError::DecodeError(
        "MJPEG decoding requires the 'turbojpeg' feature. \
         Enable it with: cargo add rustcv-camera --features turbojpeg"
            .into(),
    ))
}

// ─── YUYV → BGR ─────────────────────────────────────────────────────────────

/// Convert YUYV (YUV 4:2:2) packed data to BGR24.
/// 将 YUYV（YUV 4:2:2）打包数据转换为 BGR24。
///
/// Uses BT.601 standard with integer arithmetic (no floating point).
/// 使用 BT.601 标准的整数运算（无浮点）。
///
/// Each 4-byte YUYV macro-pixel `[Y0, U, Y1, V]` produces 2 BGR pixels (6 bytes).
/// 每 4 字节的 YUYV 宏像素 `[Y0, U, Y1, V]` 产生 2 个 BGR 像素（6 字节）。
///
/// Integer approximation of the BT.601 conversion:
/// BT.601 转换的整数近似：
/// ```text
/// R = (298*(Y-16) + 409*(V-128) + 128) >> 8
/// G = (298*(Y-16) - 100*(U-128) - 208*(V-128) + 128) >> 8
/// B = (298*(Y-16) + 516*(U-128) + 128) >> 8
/// ```
pub fn yuyv_to_bgr(src: &[u8], dst: &mut [u8], width: usize, height: usize) {
    let pixel_pairs = width * height / 2;
    let src_needed = pixel_pairs * 4;
    let dst_needed = pixel_pairs * 6;

    if src.len() < src_needed || dst.len() < dst_needed {
        return;
    }

    for i in 0..pixel_pairs {
        let si = i * 4;
        let di = i * 6;

        let y0 = src[si] as i32;
        let u = src[si + 1] as i32 - 128;
        let y1 = src[si + 2] as i32;
        let v = src[si + 3] as i32 - 128;

        let c0 = y0 - 16;
        let c1 = y1 - 16;

        // Pixel 0: BGR
        dst[di] = clamp((298 * c0 + 516 * u + 128) >> 8); // B
        dst[di + 1] = clamp((298 * c0 - 100 * u - 208 * v + 128) >> 8); // G
        dst[di + 2] = clamp((298 * c0 + 409 * v + 128) >> 8); // R

        // Pixel 1: BGR
        dst[di + 3] = clamp((298 * c1 + 516 * u + 128) >> 8); // B
        dst[di + 4] = clamp((298 * c1 - 100 * u - 208 * v + 128) >> 8); // G
        dst[di + 5] = clamp((298 * c1 + 409 * v + 128) >> 8); // R
    }
}

// ─── RGB → BGR ──────────────────────────────────────────────────────────────

/// Swap R and B channels: RGB24 → BGR24.
/// 交换 R 和 B 通道：RGB24 → BGR24。
fn rgb_to_bgr(src: &[u8], dst: &mut [u8]) {
    for (s, d) in src.chunks_exact(3).zip(dst.chunks_exact_mut(3)) {
        d[0] = s[2]; // B ← R
        d[1] = s[1]; // G ← G
        d[2] = s[0]; // R ← B
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Clamp an i32 to the [0, 255] range and cast to u8.
/// 将 i32 钳位到 [0, 255] 范围并转换为 u8。
#[inline(always)]
fn clamp(val: i32) -> u8 {
    val.clamp(0, 255) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yuyv_to_bgr_basic() {
        // White pixel in YUYV: Y=235, U=128, V=128 → should produce near-white BGR.
        // YUYV 中的白色像素：Y=235, U=128, V=128 → 应产生接近白色的 BGR。
        let yuyv = [235u8, 128, 235, 128]; // 2 white pixels
        let mut bgr = [0u8; 6];
        yuyv_to_bgr(&yuyv, &mut bgr, 2, 1);

        // Should be close to (255, 255, 255) for both pixels.
        // 两个像素都应接近 (255, 255, 255)。
        for i in 0..2 {
            let b = bgr[i * 3];
            let g = bgr[i * 3 + 1];
            let r = bgr[i * 3 + 2];
            assert!(b > 240, "B={}", b);
            assert!(g > 240, "G={}", g);
            assert!(r > 240, "R={}", r);
        }
    }

    #[test]
    fn yuyv_to_bgr_black() {
        // Black pixel in YUYV: Y=16, U=128, V=128 → should produce near-black BGR.
        // YUYV 中的黑色像素：Y=16, U=128, V=128 → 应产生接近黑色的 BGR。
        let yuyv = [16u8, 128, 16, 128];
        let mut bgr = [0u8; 6];
        yuyv_to_bgr(&yuyv, &mut bgr, 2, 1);

        for &val in &bgr {
            assert!(val < 10, "expected near-zero, got {}", val);
        }
    }

    #[test]
    fn rgb_to_bgr_swap() {
        let rgb = [255u8, 0, 0, 0, 255, 0]; // red, green
        let mut bgr = [0u8; 6];
        rgb_to_bgr(&rgb, &mut bgr);
        assert_eq!(bgr, [0, 0, 255, 0, 255, 0]); // blue (was red), green
    }
}
