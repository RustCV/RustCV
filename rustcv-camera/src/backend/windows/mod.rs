/// Windows Media Foundation camera backend — stub implementation.
/// Windows Media Foundation 摄像头后端 —— 存根实现。
///
/// This stub exists so that the crate compiles cleanly on Windows in CI/CD.
/// The full implementation (Task 12 in TASKS.md) is a future work item.
///
/// 此存根的目的是让 crate 在 CI/CD 的 Windows 环境中能够编译通过。
/// 完整实现（TASKS.md 中的 Task 12）是后续工作项。
use crate::config::{CameraConfig, ResolvedConfig};
use crate::error::{CameraError, Result};

use super::RawFrame;

/// Windows Media Foundation camera backend (not yet implemented).
/// Windows Media Foundation 摄像头后端（尚未实现）。
pub(crate) struct MsmfBackend;

impl MsmfBackend {
    /// Create a new (unimplemented) MSMF backend.
    /// 创建新的（未实现的）MSMF 后端。
    pub fn new() -> Self {
        Self
    }

    pub fn open(&mut self, _device: &str, _config: &CameraConfig) -> Result<ResolvedConfig> {
        Err(CameraError::DeviceNotFound(
            "Windows Media Foundation backend is not yet implemented".to_string(),
        ))
    }

    pub fn start(&mut self) -> Result<()> {
        Err(CameraError::StreamNotStarted)
    }

    pub fn stop(&mut self) -> Result<()> {
        Ok(())
    }

    pub fn dequeue(&mut self) -> Result<RawFrame<'_>> {
        Err(CameraError::StreamNotStarted)
    }
}
