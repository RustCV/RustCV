//! # Buffer Management
//!
//! The stream handles two types of buffer layouts:
//!
//! ## Packed Formats (YUYV, UYVY, RGB)
//!
//! Data is stored continuously with optional stride padding:
//! ```text
//! ┌─────────────────────────────────────┐
//! │ Row 0: [Pixel data][Padding]        │
//! │ Row 1: [Pixel data][Padding]        │
//! │ ...                                  │
//! └─────────────────────────────────────┘
//! ```
//!
//! ## Planar Formats (NV12, YV12)
//!
//! Data is stored in separate planes:
//! ```text
//! ┌─────────────────────────────────────┐
//! │ Y Plane: Row 0...N                  │
//! │ UV Plane: Row 0...N/2               │
//! └─────────────────────────────────────┘
//! ```
use async_trait::async_trait;
use rustcv_core::error::{CameraError, Result};
use rustcv_core::frame::{BackendBufferHandle, Frame, FrameMetadata, Timestamp};
use rustcv_core::pixel_format::{FourCC, PixelFormat};
use rustcv_core::time::ClockSynchronizer;
use rustcv_core::traits::Stream;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::Instant;
use tokio::sync::Semaphore;
use windows::core::{implement, IUnknown, Interface};
use windows::Win32::Media::MediaFoundation::{
    IMF2DBuffer2, IMFAttributes, IMFMediaEvent, IMFMediaSource, IMFSample, IMFSourceReader,
    IMFSourceReaderCallback, IMFSourceReaderCallback_Impl, MFCreateAttributes,
    MFCreateSourceReaderFromMediaSource, MF_SOURCE_READER_ASYNC_CALLBACK,
    MF_SOURCE_READER_FIRST_VIDEO_STREAM,
};

use crate::device::NegotiatedFormat;

/// Converts a Windows error to a CameraError.
fn win_err(e: windows_core::Error) -> CameraError {
    CameraError::BackendError(e.message().to_string())
}

/// Thread-safe wrapper for `IMFSample`.
///
/// Media Foundation COM objects are not inherently `Send`, but they can be safely
/// transferred between threads when proper synchronization is maintained.
#[derive(Debug, Clone)]
pub struct SendableSample(pub IMFSample);

unsafe impl Send for SendableSample {}

/// Thread-safe wrapper for `IMFSourceReader`.
///
/// The source reader is shared between the main stream and the callback,
/// requiring `Send` + `Sync` for safe concurrent access.
#[derive(Debug, Clone)]
pub struct SendableSourceReader(pub IMFSourceReader);

unsafe impl Send for SendableSourceReader {}
unsafe impl Sync for SendableSourceReader {}

/// Shared state between the callback and the stream.
///
/// This structure coordinates frame delivery from the Media Foundation callback
/// (which runs on a MF worker thread) to the async `next_frame()` method.
struct SharedState {
    /// Storage for the most recent frame sample.
    sample_slot: Mutex<Option<SendableSample>>,

    /// Hardware timestamp from the last frame (in 100ns units).
    timestamp: AtomicI64,

    /// Flag indicating whether the stream is actively capturing.
    is_running: AtomicBool,

    /// Weak reference to the source reader for requesting the next sample.
    reader: RwLock<Weak<SendableSourceReader>>,

    /// Semaphore for frame synchronization.
    frame_ready: Semaphore,
}

/// Media Foundation callback for receiving video samples.
///
/// This implements `IMFSourceReaderCallback` to receive frames asynchronously.
/// Each frame is stored in the shared state and the stream is notified.
#[implement(IMFSourceReaderCallback)]
struct MsmfCallback {
    shared: Arc<SharedState>,
}

impl MsmfCallback {
    fn new(shared: Arc<SharedState>) -> Self {
        Self { shared }
    }
}

impl IMFSourceReaderCallback_Impl for MsmfCallback_Impl {
    /// Called when a new sample is available from the source reader.
    fn OnReadSample(
        &self,
        hrstatus: windows_core::HRESULT,
        _dwstreamindex: u32,
        _dwstreamflags: u32,
        lltimestamp: i64,
        psample: windows_core::Ref<'_, IMFSample>,
    ) -> windows_core::Result<()> {
        if !self.shared.is_running.load(Ordering::Acquire) || hrstatus.is_err() {
            return Ok(());
        }

        if let Some(psample) = psample.as_ref() {
            let mut slot = self.shared.sample_slot.lock().unwrap();
            let was_empty = slot.is_none();
            *slot = Some(SendableSample(psample.clone()));
            drop(slot);

            if was_empty {
                self.shared.frame_ready.add_permits(1);
            }
        }

        self.shared.timestamp.store(lltimestamp, Ordering::Release);

        if let Some(reader) = self.shared.reader.read().unwrap().upgrade() {
            unsafe {
                let _ = reader.0.ReadSample(
                    MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32,
                    0,
                    None,
                    None,
                    None,
                    None,
                );
            }
        }
        Ok(())
    }

    fn OnFlush(&self, _: u32) -> windows_core::Result<()> {
        Ok(())
    }

    fn OnEvent(&self, _: u32, _: windows_core::Ref<'_, IMFMediaEvent>) -> windows_core::Result<()> {
        Ok(())
    }
}

/// Static handle instance for MSMF backend buffer identification.
pub static MSMF_HANDLE_INSTANCE: MsmfHandle = MsmfHandle;

/// Opaque handle type for MSMF backend buffers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MsmfHandle;

impl BackendBufferHandle for MsmfHandle {}

/// Buffer layout type for video frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BufferLayout {
    /// Packed format: pixels stored continuously (YUYV, UYVY, RGB)
    Packed,
    /// Planar format: separate Y and UV planes (NV12, YV12)
    Planar,
}

impl BufferLayout {
    /// Determines the buffer layout from a pixel format.
    fn from_format(format: PixelFormat) -> Self {
        match format {
            PixelFormat::Known(FourCC::NV12) | PixelFormat::Known(FourCC::YV12) => Self::Planar,
            _ => Self::Packed,
        }
    }
}

pub struct MsmfStream {
    /// Source reader for the media source (Arc for sharing with callback).
    source_reader: Arc<SendableSourceReader>,

    /// Shared state with the callback.
    shared: Arc<SharedState>,

    /// Linear buffer for frame data.
    linear_buffer: Vec<u8>,

    /// Frame width in pixels.
    width: u32,

    /// Frame height in pixels.
    height: u32,

    /// Pixel format of the captured frames.
    format: PixelFormat,

    /// Number of bytes per row in the frame.
    line_width_bytes: usize,

    /// Monotonically increasing frame sequence number.
    sequence: u64,

    /// Clock synchronizer for timestamp correction.
    clock_sync: ClockSynchronizer,
}

impl MsmfStream {
    pub fn new(media_source: &IMFMediaSource, fmt: NegotiatedFormat) -> Result<Self> {
        let (line_width_bytes, total_size) =
            Self::calculate_buffer_size(fmt.width, fmt.height, fmt.format);

        let shared = Arc::new(SharedState {
            sample_slot: Mutex::new(None),
            timestamp: AtomicI64::new(0),
            is_running: AtomicBool::new(false),
            reader: RwLock::new(Weak::new()),
            frame_ready: Semaphore::new(0),
        });

        let callback_impl = MsmfCallback::new(shared.clone());
        let callback_interface: IMFSourceReaderCallback = callback_impl.into();

        let attributes = unsafe {
            let mut attr: Option<IMFAttributes> = None;
            MFCreateAttributes(&mut attr, 1).map_err(win_err)?;
            let attr = attr.ok_or_else(|| {
                CameraError::Io(std::io::Error::other("Failed to create IMFAttributes"))
            })?;
            attr.SetUnknown(
                &MF_SOURCE_READER_ASYNC_CALLBACK,
                &callback_interface.cast::<IUnknown>().map_err(win_err)?,
            )
            .map_err(win_err)?;
            attr
        };

        let source_reader = unsafe {
            MFCreateSourceReaderFromMediaSource(media_source, Some(&attributes)).map_err(win_err)?
        };

        unsafe {
            source_reader
                .SetCurrentMediaType(
                    MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32,
                    None,
                    &fmt.media_type,
                )
                .map_err(win_err)?;
        }

        let reader_arc = Arc::new(SendableSourceReader(source_reader.clone()));
        let reader_weak = Arc::downgrade(&reader_arc);

        *shared.reader.write().unwrap() = reader_weak;

        Ok(Self {
            source_reader: reader_arc,
            shared,
            linear_buffer: vec![0u8; total_size],
            width: fmt.width,
            height: fmt.height,
            format: fmt.format,
            line_width_bytes,
            sequence: 0,
            clock_sync: ClockSynchronizer::new(30),
        })
    }

    /// Calculates buffer size based on pixel format.
    ///
    /// Returns a tuple of (stride, total_size) where:
    /// - stride: bytes per row
    /// - total_size: total buffer size in bytes
    fn calculate_buffer_size(width: u32, height: u32, format: PixelFormat) -> (usize, usize) {
        let h = height as usize;
        let w = width as usize;

        match format {
            PixelFormat::Known(FourCC::NV12) | PixelFormat::Known(FourCC::YV12) => {
                let y_plane = w * h;
                let uv_plane = w * h.div_ceil(2);
                (w, y_plane + uv_plane)
            }
            PixelFormat::Known(FourCC::YUYV) | PixelFormat::Known(FourCC::UYVY) => {
                let stride = w * 2;
                (stride, stride * h)
            }
            _ => {
                let bpp = format.bpp_estimate() as usize;
                let stride = w * bpp / 8;
                (stride, stride * h)
            }
        }
    }

    /// Copies frame data from an IMFSample to the linear buffer.
    fn copy_sample_to_linear_buffer(&mut self, sample: &IMFSample) -> Result<()> {
        unsafe {
            let buffer = sample.GetBufferByIndex(0).map_err(win_err)?;

            if let Ok(buffer2d) = buffer.cast::<IMF2DBuffer2>() {
                self.copy_from_2d_buffer(&buffer2d)?;
            } else {
                self.copy_from_linear_buffer(&buffer)?;
            }
        }
        Ok(())
    }

    /// Copies frame data from a 2D buffer with stride handling.
    unsafe fn copy_from_2d_buffer(&mut self, buffer: &IMF2DBuffer2) -> Result<()> {
        let (mut scanline0, mut pitch) = (std::ptr::null_mut(), 0i32);
        buffer.Lock2D(&mut scanline0, &mut pitch).map_err(win_err)?;

        let src_pitch = pitch.unsigned_abs() as usize;
        let row_bytes = self.line_width_bytes;

        let result = match BufferLayout::from_format(self.format) {
            BufferLayout::Planar => self.copy_planar_data(scanline0, pitch, src_pitch, row_bytes),
            BufferLayout::Packed => self.copy_packed_data(scanline0, pitch, src_pitch, row_bytes),
        };

        let _ = buffer.Unlock2D();
        result
    }

    /// Copies planar format data (NV12, YV12).
    unsafe fn copy_planar_data(
        &mut self,
        scanline0: *mut u8,
        pitch: i32,
        src_pitch: usize,
        row_bytes: usize,
    ) -> Result<()> {
        let y_height = self.height as usize;
        let uv_height = y_height.div_ceil(2);
        let total_height = y_height + uv_height;

        let src = self.get_src_ptr(scanline0, pitch, src_pitch, total_height);

        for row in 0..y_height {
            self.copy_row(src, row, src_pitch, row_bytes, 0);
        }

        let y_plane_size = row_bytes * y_height;
        let uv_src = src.add(y_height * src_pitch);

        for row in 0..uv_height {
            let src_row = uv_src.add(row * src_pitch);
            let dest_row = self
                .linear_buffer
                .as_mut_ptr()
                .add(y_plane_size + row * row_bytes);
            std::ptr::copy_nonoverlapping(src_row, dest_row, row_bytes);
        }

        Ok(())
    }

    /// Copies packed format data
    unsafe fn copy_packed_data(
        &mut self,
        scanline0: *mut u8,
        pitch: i32,
        src_pitch: usize,
        row_bytes: usize,
    ) -> Result<()> {
        let height = self.height as usize;

        if src_pitch == row_bytes {
            std::ptr::copy_nonoverlapping(
                scanline0,
                self.linear_buffer.as_mut_ptr(),
                row_bytes * height,
            );
        } else {
            let src = self.get_src_ptr(scanline0, pitch, src_pitch, height);

            for row in 0..height {
                self.copy_row(src, row, src_pitch, row_bytes, 0);
            }
        }

        Ok(())
    }

    /// Gets the source pointer, handling negative pitch (bottom-up images).
    #[inline]
    unsafe fn get_src_ptr(
        &self,
        scanline0: *mut u8,
        pitch: i32,
        src_pitch: usize,
        total_height: usize,
    ) -> *const u8 {
        if pitch < 0 {
            scanline0.sub((total_height - 1) * src_pitch)
        } else {
            scanline0
        }
    }

    /// Copies a single row from source to destination.
    #[inline]
    unsafe fn copy_row(
        &mut self,
        src: *const u8,
        row: usize,
        src_pitch: usize,
        row_bytes: usize,
        dest_offset: usize,
    ) {
        let src_row = src.add(row * src_pitch);
        let dest_row = self
            .linear_buffer
            .as_mut_ptr()
            .add(dest_offset + row * row_bytes);
        std::ptr::copy_nonoverlapping(src_row, dest_row, row_bytes);
    }

    /// Copies frame data from a linear/contiguous buffer.
    unsafe fn copy_from_linear_buffer(
        &mut self,
        buffer: &windows::Win32::Media::MediaFoundation::IMFMediaBuffer,
    ) -> Result<()> {
        let (mut ptr, mut len) = (std::ptr::null_mut(), 0);
        buffer
            .Lock(&mut ptr, None, Some(&mut len))
            .map_err(win_err)?;

        let copy_len = (len as usize).min(self.linear_buffer.len());
        std::ptr::copy_nonoverlapping(ptr, self.linear_buffer.as_mut_ptr(), copy_len);

        let _ = buffer.Unlock();
        Ok(())
    }

    /// Returns a clone of the source reader for control interfaces.
    pub fn get_reader(&self) -> SendableSourceReader {
        SendableSourceReader(self.source_reader.0.clone())
    }
}

#[async_trait]
impl Stream for MsmfStream {
    /// Starts the video capture stream.
    ///
    /// This initiates the first sample request, which triggers the callback pipeline.
    async fn start(&mut self) -> Result<()> {
        self.shared.is_running.store(true, Ordering::Release);

        unsafe {
            self.source_reader
                .0
                .ReadSample(
                    MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32,
                    0,
                    None,
                    None,
                    None,
                    None,
                )
                .map_err(win_err)?;
        }

        Ok(())
    }

    /// Stops the video capture stream.
    ///
    /// Flushes any pending samples and prevents further callbacks.
    async fn stop(&mut self) -> Result<()> {
        self.shared.is_running.store(false, Ordering::Release);

        unsafe {
            let _ = self
                .source_reader
                .0
                .Flush(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32);
        }

        Ok(())
    }

    /// Waits for and returns the next available frame.
    ///
    /// This method uses a semaphore to efficiently wait for new frames
    /// without busy-waiting.
    async fn next_frame(&mut self) -> Result<Frame<'_>> {
        let sample = self.wait_for_sample().await?;
        let ts_raw = self.shared.timestamp.load(Ordering::Acquire);

        self.copy_sample_to_linear_buffer(&sample.0)?;

        self.sequence += 1;
        let hw_ns = (ts_raw as u64) * 100;

        Ok(Frame {
            data: &self.linear_buffer,
            width: self.width,
            height: self.height,
            stride: self.line_width_bytes,
            format: self.format,
            sequence: self.sequence,
            timestamp: Timestamp {
                hw_raw_ns: hw_ns,
                system_synced: self.clock_sync.correct(hw_ns, Instant::now()),
            },
            metadata: FrameMetadata::default(),
            backend_handle: &MSMF_HANDLE_INSTANCE,
        })
    }
}

impl MsmfStream {
    /// Waits for a sample to become available and returns it.
    ///
    /// Uses a semaphore to track available frames. The semaphore's permits
    /// accumulate when frames arrive, ensuring no notifications are missed.
    async fn wait_for_sample(&self) -> Result<SendableSample> {
        let permit = self.shared.frame_ready.acquire().await.map_err(|e| {
            CameraError::Io(std::io::Error::other(format!("Semaphore error: {}", e)))
        })?;
        permit.forget();

        let mut slot = self.shared.sample_slot.lock().unwrap();
        slot.take()
            .ok_or_else(|| CameraError::Io(std::io::Error::other("Frame slot was empty")))
    }
}
