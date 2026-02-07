// src/stream.rs
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use objc2::rc::Retained;
use objc2_av_foundation::{
    AVCaptureDevice, AVCaptureDeviceInput, AVCaptureSession, AVCaptureSessionPreset640x480,
    AVCaptureVideoDataOutput,
};
use objc2_foundation::{NSNumber, NSString};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

use rustcv_core::error::CameraError;
use rustcv_core::frame::{BackendBufferHandle, Frame, FrameMetadata, Timestamp};
use rustcv_core::pixel_format::FourCC;
use rustcv_core::traits::Stream;

use crate::delegate::{AvfFrameData, CaptureDelegate};

#[derive(Debug)]
pub struct AvfBufferHandle;
impl BackendBufferHandle for AvfBufferHandle {}
static AVF_HANDLE: AvfBufferHandle = AvfBufferHandle;

pub struct AvfStream {
    session: Retained<AVCaptureSession>,
    // 需要持有这些对象以维持引用计数
    _delegate: Retained<CaptureDelegate>,
    _input: Retained<AVCaptureDeviceInput>,
    _output: Retained<AVCaptureVideoDataOutput>,

    receiver: UnboundedReceiver<AvfFrameData>,
    current_frame: Option<AvfFrameData>,

    is_streaming: bool,
}

unsafe impl Send for AvfStream {}

impl AvfStream {
    pub fn new(device_id: &str) -> Result<Self> {
        unsafe {
            let session = AVCaptureSession::new();
            session.setSessionPreset(AVCaptureSessionPreset640x480);

            // 1. 查找设备
            let device = AVCaptureDevice::deviceWithUniqueID(&NSString::from_str(device_id))
                .ok_or_else(|| anyhow!("Device ID not found: {}", device_id))?;

            // 2. 创建 Input
            // 注意：API 可能会返回 Option 或者 Result
            let input = AVCaptureDeviceInput::deviceInputWithDevice_error(&device)
                .map_err(|e| anyhow!("Failed to create input: {:?}", e))?;

            if session.canAddInput(&input) {
                session.addInput(&input);
            } else {
                return Err(anyhow!("Cannot add input to session"));
            }

            // 3. 创建 Output
            let output = AVCaptureVideoDataOutput::new();

            // 设置 '2vuy' (YUYV) 格式
            let key = NSString::from_str("PixelFormatType");
            let val = NSNumber::new_u32(846624121);

            use objc2::runtime::{AnyObject, ProtocolObject};
            use objc2_foundation::{NSCopying, NSDictionary, NSObjectProtocol};

            // Cast key to ProtocolObject<dyn NSCopying>
            // We use unsafe transmute because NSString strictly implements NSCopying
            // and ProtocolObject is a transparent wrapper around Id.
            let key_proto: &ProtocolObject<dyn NSCopying> = std::mem::transmute(&*key);

            // Cast val to AnyObject (ProtocolObject<dyn NSObjectProtocol>)
            // NSNumber implements NSObjectProtocol.
            let val_proto: &AnyObject =
                std::mem::transmute(ProtocolObject::<dyn NSObjectProtocol>::from_ref(&*val));

            // Create immutable dictionary
            let settings = NSDictionary::<NSString, AnyObject>::dictionaryWithObject_forKey(
                val_proto, key_proto,
            );

            output.setVideoSettings(Some(&settings));

            // 4. 连接 Delegate
            let (tx, rx) = unbounded_channel();
            let delegate = CaptureDelegate::new(tx);
            let queue = crate::gcd::get_global_queue();

            // AVCaptureVideoDataOutputSampleBufferDelegate protocol wrapper
            let delegate_proto = ProtocolObject::from_ref(&*delegate);

            // dispatch2::Queue
            output.setSampleBufferDelegate_queue(Some(delegate_proto), Some(queue));

            if session.canAddOutput(&output) {
                session.addOutput(&output);
            } else {
                return Err(anyhow!("Cannot add output to session"));
            }

            // 提交配置
            session.commitConfiguration();

            Ok(Self {
                session,
                _delegate: delegate,
                _input: input,
                _output: output,
                receiver: rx,
                current_frame: None,
                is_streaming: false,
            })
        }
    }
}

#[async_trait]
impl Stream for AvfStream {
    async fn start(&mut self) -> Result<(), CameraError> {
        unsafe {
            self.session.startRunning();
        }
        self.is_streaming = true;
        Ok(())
    }

    async fn stop(&mut self) -> Result<(), CameraError> {
        unsafe {
            self.session.stopRunning();
        }
        self.is_streaming = false;
        Ok(())
    }

    async fn next_frame(&mut self) -> Result<Frame<'_>, CameraError> {
        if !self.is_streaming {
            return Err(CameraError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Stream not started",
            )));
        }

        let frame_data = self.receiver.recv().await.ok_or_else(|| {
            CameraError::Io(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "Stream closed",
            ))
        })?;

        self.current_frame = Some(frame_data);
        let f = self.current_frame.as_ref().unwrap();

        Ok(Frame {
            data: &f.data,
            width: f.width as u32,
            height: f.height as u32,
            stride: f.width * 2, // YUYV approx
            format: FourCC::YUYV.into(),
            sequence: 0,
            timestamp: Timestamp {
                hw_raw_ns: 0,
                system_synced: std::time::Duration::ZERO,
            },
            metadata: FrameMetadata::default(),
            backend_handle: &AVF_HANDLE,
        })
    }

    #[cfg(feature = "simulation")]
    async fn inject_frame(&mut self, _: Frame<'_>) -> Result<(), CameraError> {
        Ok(())
    }
}
