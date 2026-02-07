use std::sync::Arc;
use std::time::Instant;
use windows::Win32::Media::MediaFoundation::*;

use async_trait::async_trait;

use rustcv_core::error::{CameraError, Result};
use rustcv_core::frame::{BackendBufferHandle, Frame, FrameMetadata, Timestamp};
use rustcv_core::time::ClockSynchronizer;
use rustcv_core::traits::Stream;

#[derive(Debug)]
pub struct MsmfBufferHandle;
impl BackendBufferHandle for MsmfBufferHandle {}

static MSMF_HANDLE_INSTANCE: MsmfBufferHandle = MsmfBufferHandle;

pub struct MsmfStream {
    source_reader: Arc<IMFSourceReader>,
    width: u32,
    height: u32,
    format: rustcv_core::pixel_format::PixelFormat,
    clock_sync: ClockSynchronizer,
    is_streaming: bool,
    sequence: u64,
    frame_data: Vec<u8>,
    stride: usize,
}

unsafe impl Send for MsmfStream {}

impl MsmfStream {
    pub fn new(
        source_reader: Arc<IMFSourceReader>,
        fmt: &super::device::NegotiatedFormat,
        _buf_count: usize,
    ) -> Result<Self> {
        let stride = (fmt.width * fmt.format.bpp_estimate() / 8) as usize;
        let estimated_size = stride * fmt.height as usize;

        Ok(Self {
            source_reader,
            width: fmt.width,
            height: fmt.height,
            format: fmt.format,
            clock_sync: ClockSynchronizer::new(30),
            is_streaming: false,
            sequence: 0,
            frame_data: Vec::with_capacity(estimated_size),
            stride,
        })
    }

    fn hresult_to_camera_error(e: windows::core::Error) -> CameraError {
        CameraError::Io(std::io::Error::other(e.to_string()))
    }

    unsafe fn extract_frame_data(&mut self, media_buffer: &IMFMediaBuffer) -> Result<()> {
        let mut data_ptr = std::ptr::null_mut();
        let mut current_length = 0u32;
        let mut max_length = 0u32;

        media_buffer
            .Lock(
                &mut data_ptr,
                Some(&mut max_length),
                Some(&mut current_length),
            )
            .map_err(Self::hresult_to_camera_error)?;

        if self.frame_data.capacity() < current_length as usize {
            self.frame_data
                .reserve(current_length as usize - self.frame_data.capacity());
        }

        self.frame_data.set_len(current_length as usize);
        std::ptr::copy_nonoverlapping(
            data_ptr as *const u8,
            self.frame_data.as_mut_ptr(),
            current_length as usize,
        );

        media_buffer
            .Unlock()
            .map_err(Self::hresult_to_camera_error)?;
        Ok(())
    }
}

#[async_trait]
impl Stream for MsmfStream {
    async fn start(&mut self) -> Result<()> {
        self.is_streaming = true;
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        self.is_streaming = false;
        Ok(())
    }

    async fn next_frame(&mut self) -> Result<Frame<'_>> {
        if !self.is_streaming {
            return Err(CameraError::Io(std::io::Error::other("Stream not started")));
        }
        let mut stream_index = 0u32;
        let mut flags = 0u32;
        let mut timestamp = 0i64;
        let mut sample = None;

        loop {
            unsafe {
                self.source_reader
                    .ReadSample(
                        MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32,
                        0u32,
                        Some(&mut stream_index),
                        Some(&mut flags),
                        Some(&mut timestamp),
                        Some(&mut sample),
                    )
                    .map_err(Self::hresult_to_camera_error)?;
            }
            if sample.is_some() {
                break;
            }
        }

        let sample =
            sample.ok_or_else(|| CameraError::Io(std::io::Error::other("No sample received")))?;

        let media_buffer = unsafe {
            sample
                .GetBufferByIndex(0)
                .map_err(Self::hresult_to_camera_error)?
        };

        unsafe { self.extract_frame_data(&media_buffer)? };

        let arrival_time = Instant::now();
        let hw_ns = (timestamp as u64) * 100;
        let synced_time = self.clock_sync.correct(hw_ns, arrival_time);

        let metadata = FrameMetadata {
            actual_exposure_us: None,
            actual_gain_db: None,
            trigger_fired: false,
            strobe_active: false,
        };

        self.sequence += 1;

        let frame = Frame {
            data: &self.frame_data,
            width: self.width,
            height: self.height,
            stride: self.stride,
            format: self.format,
            sequence: self.sequence,
            timestamp: Timestamp {
                hw_raw_ns: hw_ns,
                system_synced: synced_time,
            },
            metadata,
            backend_handle: &MSMF_HANDLE_INSTANCE,
        };

        Ok(frame)
    }

    #[cfg(feature = "simulation")]
    async fn inject_frame(&mut self, _frame: Frame<'_>) -> Result<()> {
        Err(CameraError::SimulationError(
            "Not supported on real MSMF hardware".into(),
        ))
    }
}
