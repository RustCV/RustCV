use std::io;
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use v4l::buffer::Type;

// 【关键修复】同时引入 Stream (用于 start/stop) 和 CaptureStream (用于 next)
use v4l::io::traits::{CaptureStream, Stream as V4lStream};

use rustcv_core::error::{CameraError, Result};
use rustcv_core::frame::{BackendBufferHandle, Frame, FrameMetadata, Timestamp};
use rustcv_core::time::ClockSynchronizer;
use rustcv_core::traits::Stream; // 这里是 rustcv 定义的 trait

// 本地句柄结构体，解决孤儿规则
#[derive(Debug)]
pub struct V4l2BufferHandle;
impl BackendBufferHandle for V4l2BufferHandle {}

// 静态实例
static V4L2_HANDLE_INSTANCE: V4l2BufferHandle = V4l2BufferHandle;

pub struct V4l2Stream {
    inner: v4l::io::mmap::Stream<'static>,
    format: v4l::Format,
    clock_sync: ClockSynchronizer,
    is_streaming: bool,
    _dev: Arc<v4l::Device>,
}

unsafe impl Send for V4l2Stream {}

impl V4l2Stream {
    pub fn new(dev: Arc<v4l::Device>, fmt: &v4l::Format, buf_count: usize) -> Result<Self> {
        let stream =
            v4l::io::mmap::Stream::with_buffers(&dev, Type::VideoCapture, buf_count as u32)
                .map_err(CameraError::Io)?;

        Ok(Self {
            inner: stream,
            format: *fmt,
            clock_sync: ClockSynchronizer::new(30),
            is_streaming: false,
            _dev: dev,
        })
    }
}

#[async_trait]
impl Stream for V4l2Stream {
    async fn start(&mut self) -> Result<()> {
        // 调用 v4l::io::traits::Stream 的 start
        V4lStream::start(&mut self.inner).map_err(CameraError::Io)?;
        self.is_streaming = true;
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        // 调用 v4l::io::traits::Stream 的 stop
        V4lStream::stop(&mut self.inner).map_err(CameraError::Io)?;
        self.is_streaming = false;
        Ok(())
    }

    async fn next_frame(&mut self) -> Result<Frame<'_>> {
        if !self.is_streaming {
            return Err(CameraError::Io(io::Error::other("Stream not started")));
        }

        // 调用 CaptureStream 的 next
        let (buf, meta) = self.inner.next().map_err(CameraError::Io)?;
        let arrival_time = Instant::now();

        let hw_ns =
            (meta.timestamp.sec as u64 * 1_000_000_000) + (meta.timestamp.usec as u64 * 1_000);

        let synced_time = self.clock_sync.correct(hw_ns, arrival_time);

        let metadata = FrameMetadata {
            actual_exposure_us: None,
            actual_gain_db: None,
            trigger_fired: false,
            strobe_active: false,
        };

        let frame = Frame {
            data: buf,
            width: self.format.width,
            height: self.format.height,
            stride: meta.bytesused as usize / self.format.height as usize,
            format: crate::pixel_map::from_v4l_fourcc(self.format.fourcc),
            sequence: meta.sequence as u64,
            timestamp: Timestamp {
                hw_raw_ns: hw_ns,
                system_synced: synced_time,
            },
            metadata,
            backend_handle: &V4L2_HANDLE_INSTANCE,
        };

        Ok(frame)
    }

    #[cfg(feature = "simulation")]
    async fn inject_frame(&mut self, _frame: Frame<'_>) -> Result<()> {
        Err(CameraError::SimulationError(
            "Not supported on real V4L2 hardware".into(),
        ))
    }
}
