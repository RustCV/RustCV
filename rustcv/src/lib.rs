pub mod core;
pub mod highgui;
pub mod imgcodecs;
pub mod imgproc;
pub(crate) mod internal;
pub mod videoio; // 内部模块，不对外暴露

// Re-export 核心类型，方便 prelude 使用
pub use core::mat::Mat;

/// 预置模块，用户可以通过 `use rustcv::prelude::*;` 导入常用项
pub mod prelude {
    pub use crate::core::mat::Mat;
    pub use crate::videoio::VideoCapture;
    // 未来添加更多...
}
