use thiserror::Error;

#[derive(Error, Debug)]
pub enum CameraError {
    #[error("Device disconnected: {0}")]
    Disconnected(String),

    #[error("USB Bandwidth exceeded. Suggested action: {suggestion}")]
    BandwidthExceeded {
        required_mbps: u32,
        limit_mbps: u32,
        suggestion: String, // e.g., "Try MJPEG format"
    },

    #[error("Device busy: Exclusive access required")]
    DeviceBusy,

    #[error("Frame dropped due to ring buffer overflow")]
    BufferOverflow,

    #[error("Format negotiation failed: No hardware support for requested constraints")]
    FormatNotSupported,

    #[error("Simulation backend error: {0}")]
    SimulationError(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, CameraError>;
