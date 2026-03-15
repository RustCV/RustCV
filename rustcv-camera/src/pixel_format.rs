/// Pixel format representation using V4L2 FourCC conventions.
/// 基于 V4L2 FourCC 约定的像素格式表示。
///
/// FourCC is a 4-byte code packed into a `u32`, where each byte is an ASCII character.
/// For example, MJPG = `b'M' | (b'J' << 8) | (b'P' << 16) | (b'G' << 24)`.
/// FourCC 是将 4 个 ASCII 字符打包到 `u32` 中的编码方式。
/// 例如 MJPG = `b'M' | (b'J' << 8) | (b'P' << 16) | (b'G' << 24)`。
///
/// Common pixel formats for camera capture.
/// 摄像头采集中常见的像素格式。
///
/// Using an enum (instead of raw `u32`) provides:
/// - Exhaustive match checking at compile time
/// - Type safety (cannot accidentally pass a width as a format)
/// - `Other(u32)` preserves extensibility for unknown formats
///
/// 使用枚举（而非原始 `u32`）的好处：
/// - 编译时穷举检查
/// - 类型安全（不会把宽度误传为格式）
/// - `Other(u32)` 保留了对未知格式的扩展性
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// Motion JPEG compressed format.
    /// Most common compressed format for USB cameras.
    /// Low bandwidth, requires CPU decoding.
    ///
    /// Motion JPEG 压缩格式。
    /// USB 摄像头最常见的压缩格式。带宽低，需要 CPU 解码。
    Mjpeg,

    /// YUYV 4:2:2 uncompressed format.
    /// Most common uncompressed format for USB cameras.
    /// 2 bytes per pixel, simple CPU conversion to BGR.
    ///
    /// YUYV 4:2:2 无压缩格式。
    /// USB 摄像头最常见的无压缩格式。每像素 2 字节，CPU 转换为 BGR 简单。
    Yuyv,

    /// NV12 (YUV 4:2:0 semi-planar) format.
    /// Common on mobile/embedded devices.
    /// 1.5 bytes per pixel.
    ///
    /// NV12（YUV 4:2:0 半平面）格式。
    /// 常见于手机/嵌入式设备。每像素 1.5 字节。
    Nv12,

    /// BGR 24-bit format (Blue-Green-Red).
    /// OpenCV's default channel order. Already decoded, no conversion needed.
    ///
    /// BGR 24 位格式（蓝-绿-红）。
    /// OpenCV 默认的通道顺序。已解码，无需转换。
    Bgr24,

    /// RGB 24-bit format (Red-Green-Blue).
    /// Standard display order. Needs channel swap for OpenCV compatibility.
    ///
    /// RGB 24 位格式（红-绿-蓝）。
    /// 标准显示顺序。需要通道交换以兼容 OpenCV。
    Rgb24,

    /// Unknown or unsupported format, stored as raw FourCC value.
    /// 未知或不支持的格式，存储为原始 FourCC 值。
    Other(u32),
}

/// Helper macro to build a FourCC u32 from 4 ASCII bytes.
/// 辅助宏：从 4 个 ASCII 字节构造 FourCC u32 值。
const fn fourcc(a: u8, b: u8, c: u8, d: u8) -> u32 {
    (a as u32) | ((b as u32) << 8) | ((c as u32) << 16) | ((d as u32) << 24)
}

/// Well-known FourCC constants matching V4L2 / kernel definitions.
/// 与 V4L2 / 内核定义一致的常用 FourCC 常量。
pub mod fcc {
    use super::fourcc;
    pub const MJPEG: u32 = fourcc(b'M', b'J', b'P', b'G');
    pub const YUYV: u32 = fourcc(b'Y', b'U', b'Y', b'V');
    pub const NV12: u32 = fourcc(b'N', b'V', b'1', b'2');
    pub const BGR24: u32 = fourcc(b'B', b'G', b'R', b'3');
    pub const RGB24: u32 = fourcc(b'R', b'G', b'B', b'3');
}

impl PixelFormat {
    /// Convert a V4L2 FourCC `u32` to a [`PixelFormat`].
    /// 将 V4L2 FourCC `u32` 转换为 [`PixelFormat`]。
    pub fn from_fourcc(fourcc: u32) -> Self {
        match fourcc {
            fcc::MJPEG => Self::Mjpeg,
            fcc::YUYV => Self::Yuyv,
            fcc::NV12 => Self::Nv12,
            fcc::BGR24 => Self::Bgr24,
            fcc::RGB24 => Self::Rgb24,
            other => Self::Other(other),
        }
    }

    /// Convert back to a V4L2 FourCC `u32`.
    /// 转换回 V4L2 FourCC `u32`。
    pub fn to_fourcc(self) -> u32 {
        match self {
            Self::Mjpeg => fcc::MJPEG,
            Self::Yuyv => fcc::YUYV,
            Self::Nv12 => fcc::NV12,
            Self::Bgr24 => fcc::BGR24,
            Self::Rgb24 => fcc::RGB24,
            Self::Other(v) => v,
        }
    }

    /// Returns a human-readable FourCC string (e.g., "MJPG").
    /// 返回人类可读的 FourCC 字符串（如 "MJPG"）。
    pub fn fourcc_str(self) -> String {
        let v = self.to_fourcc();
        let bytes = [
            (v & 0xFF) as u8,
            ((v >> 8) & 0xFF) as u8,
            ((v >> 16) & 0xFF) as u8,
            ((v >> 24) & 0xFF) as u8,
        ];
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

impl std::fmt::Display for PixelFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.fourcc_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fourcc_roundtrip() {
        // Verify that from_fourcc and to_fourcc are inverse operations.
        // 验证 from_fourcc 和 to_fourcc 互为逆操作。
        let formats = [
            (PixelFormat::Mjpeg, fcc::MJPEG, "MJPG"),
            (PixelFormat::Yuyv, fcc::YUYV, "YUYV"),
            (PixelFormat::Nv12, fcc::NV12, "NV12"),
        ];
        for (fmt, expected_fcc, expected_str) in formats {
            assert_eq!(fmt.to_fourcc(), expected_fcc);
            assert_eq!(PixelFormat::from_fourcc(expected_fcc), fmt);
            assert_eq!(fmt.fourcc_str(), expected_str);
        }
    }

    #[test]
    fn unknown_fourcc_preserved() {
        // Unknown FourCC values should round-trip through Other(u32).
        // 未知的 FourCC 值应通过 Other(u32) 完成往返转换。
        let unknown = 0xDEADBEEF;
        let fmt = PixelFormat::from_fourcc(unknown);
        assert_eq!(fmt, PixelFormat::Other(unknown));
        assert_eq!(fmt.to_fourcc(), unknown);
    }
}
