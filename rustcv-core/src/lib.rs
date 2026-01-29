// 开启一些 Clippy 检查，保证代码质量
#![warn(missing_debug_implementations, rust_2018_idioms, unreachable_pub)]

// 模块定义
pub mod builder;
pub mod error;
pub mod frame;
pub mod pixel_format;
pub mod telemetry;
pub mod time;
pub mod traits;

// 方便用户使用的 Prelude
pub mod prelude {
    pub use crate::builder::{CameraConfig, Priority};
    pub use crate::error::{CameraError, Result};
    pub use crate::frame::{Frame, FrameMetadata};
    pub use crate::traits::{DeviceControls, Driver, Stream};

    #[cfg(unix)]
    pub use crate::frame::AsDmaBuf;

    #[cfg(windows)]
    pub use crate::frame::AsDxResource;
}

// 重新导出依赖中的关键类型，避免用户版本冲突
pub use async_trait::async_trait;
pub use futures_core::Stream as FuturesStream;

// 版本与构建信息常量
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
