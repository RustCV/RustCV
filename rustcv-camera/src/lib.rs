//! `rustcv-camera` — Cross-platform camera capture with zero-copy frame access.
//! `rustcv-camera` —— 跨平台摄像头采集，提供零拷贝帧访问。
//!
//! # Two API styles
//! # 两种 API 风格
//!
//! ## Rust-idiomatic (zero-copy)
//! ## Rust 惯用风格（零拷贝）
//!
//! ```no_run
//! use rustcv_camera::Camera;
//!
//! let mut cam = Camera::open(0).unwrap();
//! while let Ok(frame) = cam.next_frame() {
//!     // frame.data() is zero-copy: points directly to kernel mmap buffer
//!     // frame.data() 是零拷贝：直接指向内核 mmap 缓冲区
//!     println!("{}x{} {} bytes", frame.width(), frame.height(), frame.data().len());
//! }
//! ```
//!
//! ## OpenCV-compatible (auto-decode)
//! ## 兼容 OpenCV 风格（自动解码）
//!
//! ```no_run
//! use rustcv_camera::{VideoCapture, Mat};
//!
//! let mut cap = VideoCapture::open(0).unwrap();
//! let mut frame = Mat::new();
//! while cap.read(&mut frame).unwrap() {
//!     // frame is decoded BGR, ready for processing
//!     // frame 已解码为 BGR，可直接处理
//!     println!("{}x{}", frame.cols(), frame.rows());
//! }
//! ```

// ─── Modules ────────────────────────────────────────────────────────────────

pub(crate) mod backend;
mod camera;
mod config;
mod decode;
mod error;
mod frame;
mod mat;
mod pixel_format;
mod videocapture;

// ─── Public re-exports ──────────────────────────────────────────────────────

pub use camera::Camera;
pub use config::{CameraConfig, ResolvedConfig};
pub use error::{CameraError, Result};
pub use frame::{Frame, OwnedFrame};
pub use mat::Mat;
pub use pixel_format::PixelFormat;
pub use videocapture::VideoCapture;
