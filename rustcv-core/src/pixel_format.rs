use std::fmt::{self, Display};

/// 四字符代码 (Four Character Code)，视频工业标准
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct FourCC(pub u32);

impl FourCC {
    /// 从 ASCII 字符创建 FourCC
    pub const fn new(a: u8, b: u8, c: u8, d: u8) -> Self {
        Self((a as u32) | ((b as u32) << 8) | ((c as u32) << 16) | ((d as u32) << 24))
    }

    // 获取人类可读的字符串表示 (例如 "YUYV")
    // pub fn to_string(&self) -> String {
    //     let bytes = self.0.to_le_bytes();
    //     String::from_utf8_lossy(&bytes).to_string()
    // }
}

impl Display for FourCC {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bytes = self.0.to_le_bytes();

        write!(f, "{}", String::from_utf8_lossy(&bytes))
    }
}

impl fmt::Debug for FourCC {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FourCC({})", self)
    }
}

/// 常用像素格式定义
impl FourCC {
    // --- YUV Formats ---
    /// YUYV 4:2:2 - 工业相机最常用的未压缩格式
    pub const YUYV: Self = Self::new(b'Y', b'U', b'Y', b'V');
    /// UYVY 4:2:2
    pub const UYVY: Self = Self::new(b'U', b'Y', b'V', b'Y');
    /// NV12 4:2:0 - GPU 和编码器偏好的格式
    pub const NV12: Self = Self::new(b'N', b'V', b'1', b'2');
    /// YV12 4:2:0 (Planar)
    pub const YV12: Self = Self::new(b'Y', b'V', b'1', b'2');

    // --- RGB Formats ---
    /// RGB24 (Little Endian: B-G-R)
    pub const BGR3: Self = Self::new(b'B', b'G', b'R', b'3');
    /// RGB24 (Big Endian: R-G-B)
    pub const RGB3: Self = Self::new(b'R', b'G', b'B', b'3');
    /// RGBA32
    pub const RGBA: Self = Self::new(b'R', b'G', b'B', b'A');

    // --- Compressed Formats ---
    /// Motion-JPEG - 用于节省 USB 带宽
    pub const MJPEG: Self = Self::new(b'M', b'J', b'P', b'G');
    /// H.264
    pub const H264: Self = Self::new(b'H', b'2', b'6', b'4');

    // --- Bayer Formats (Raw Sensor Data) ---
    // 命名规则通常遵循 V4L2: BA81 = BGGR8, etc.
    // 这里的 FourCC 可能需要根据具体后端 (V4L2 vs MF) 做微调，这里使用通用定义。

    /// Raw Bayer BGGR 8-bit
    pub const BA81: Self = Self::new(b'B', b'A', b'8', b'1');
    /// Raw Bayer GBRG 8-bit
    pub const GBRG: Self = Self::new(b'G', b'B', b'R', b'G');
    /// Raw Bayer GRBG 8-bit
    pub const GRBG: Self = Self::new(b'G', b'R', b'B', b'G');
    /// Raw Bayer RGGB 8-bit
    pub const RGGB: Self = Self::new(b'R', b'G', b'G', b'B');

    // --- Depth Formats ---
    /// 16-bit Depth (Z16)
    pub const Z16: Self = Self::new(b'Z', b'1', b'6', b' ');
}

/// 像素格式的高级枚举，包含元数据
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// 已知的标准格式
    Known(FourCC),
    /// 驱动返回了库不认识的私有格式
    Unknown(u32),
}

impl PixelFormat {
    /// 判断是否为压缩格式 (JPEG, H264)
    pub fn is_compressed(&self) -> bool {
        match self {
            Self::Known(cc) => matches!(*cc, FourCC::MJPEG | FourCC::H264),
            _ => false,
        }
    }

    /// 判断是否为 Bayer 原始格式 (需要 Demosaic)
    pub fn is_bayer(&self) -> bool {
        match self {
            Self::Known(cc) => matches!(
                *cc,
                FourCC::BA81 | FourCC::GBRG | FourCC::GRBG | FourCC::RGGB
            ),
            _ => false,
        }
    }

    /// 估算每像素比特数 (Bits Per Pixel)，用于计算带宽
    pub fn bpp_estimate(&self) -> u32 {
        match self {
            Self::Known(cc) => match *cc {
                FourCC::YUYV | FourCC::UYVY => 16,
                FourCC::BGR3 | FourCC::RGB3 => 24,
                FourCC::RGBA => 32,
                FourCC::NV12 | FourCC::YV12 => 12, // 平均 12 bpp
                FourCC::Z16 => 16,
                // Bayer 8-bit
                FourCC::BA81 | FourCC::GBRG | FourCC::GRBG | FourCC::RGGB => 8,
                // 压缩格式无法准确估算，给一个典型值
                FourCC::MJPEG | FourCC::H264 => 4,
                _ => 0,
            },
            _ => 0,
        }
    }
}

impl From<u32> for PixelFormat {
    fn from(val: u32) -> Self {
        // 这里可以维护一个已知列表的查找
        // 简化起见，我们假设只要是上面定义的常量都算 Known
        // 在实际工程中，这里会有一个 match 匹配所有常量
        Self::Known(FourCC(val))
    }
}

impl From<FourCC> for PixelFormat {
    fn from(cc: FourCC) -> Self {
        Self::Known(cc)
    }
}

impl PartialEq<PixelFormat> for FourCC {
    fn eq(&self, other: &PixelFormat) -> bool {
        match other {
            PixelFormat::Known(cc) => self == cc,
            PixelFormat::Unknown(val) => self.0 == *val,
        }
    }
}

// 反向比较也加上
impl PartialEq<FourCC> for PixelFormat {
    fn eq(&self, other: &FourCC) -> bool {
        match self {
            PixelFormat::Known(cc) => cc == other,
            PixelFormat::Unknown(val) => *val == other.0,
        }
    }
}
