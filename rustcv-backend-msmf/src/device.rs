use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock};
use windows::core::{Error as HResultError, GUID, HSTRING};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::*;

use rustcv_core::builder::CameraConfig;
use rustcv_core::error::{CameraError, Result};
use rustcv_core::pixel_format::PixelFormat;
use rustcv_core::traits::{DeviceControls, DeviceInfo, Stream};

use crate::controls::create_controls;
use crate::pixel_map;
use crate::stream::MsmfStream;

static INITIALIZED: LazyLock<Arc<AtomicBool>> = LazyLock::new(|| Arc::new(AtomicBool::new(false)));
static CAMERA_REFCNT: LazyLock<Arc<AtomicUsize>> = LazyLock::new(|| Arc::new(AtomicUsize::new(0)));

fn hresult_to_camera_error(e: HResultError) -> CameraError {
    CameraError::Io(std::io::Error::other(e.to_string()))
}

pub fn list_devices() -> Result<Vec<DeviceInfo>> {
    initialize_mf()?;

    unsafe {
        let mut attributes = None;
        MFCreateAttributes(&mut attributes, 1).map_err(hresult_to_camera_error)?;
        let attributes = attributes
            .ok_or_else(|| CameraError::Io(std::io::Error::other("Failed to create attributes")))?;

        attributes
            .SetGUID(
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
            )
            .map_err(hresult_to_camera_error)?;

        let mut pp_devices: MaybeUninit<*mut Option<IMFActivate>> = MaybeUninit::uninit();
        let mut count: u32 = 0;
        MFEnumDeviceSources(&attributes, pp_devices.as_mut_ptr(), &mut count)
            .map_err(hresult_to_camera_error)?;

        let ptr = pp_devices.assume_init();
        let devices = if count > 0 && !ptr.is_null() {
            let slice = std::slice::from_raw_parts_mut(ptr, count as usize);
            let result: Vec<IMFActivate> = slice.iter_mut().filter_map(|opt| opt.take()).collect();
            CoTaskMemFree(Some(ptr as *const std::ffi::c_void));
            result
        } else {
            Vec::new()
        };

        Ok(devices
            .into_iter()
            .filter_map(|device| {
                let name = get_attribute_string(&device, MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME);
                if name.is_empty() {
                    None
                } else {
                    Some(DeviceInfo {
                        name,
                        id: get_attribute_string(
                            &device,
                            MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
                        ),
                        backend: "MSMF".to_string(),
                        bus_info: None,
                    })
                }
            })
            .collect())
    }
}

/// Retrieves a string attribute from a device's `IMFActivate` interface.
unsafe fn get_attribute_string(attr: &IMFActivate, guid: GUID) -> String {
    let mut value = windows::core::PWSTR::null();
    let mut length = 0;

    // The `GetAllocatedString` function allocates memory that we must free later.
    if attr
        .GetAllocatedString(&guid, &mut value, &mut length)
        .is_ok()
        && !value.is_null()
    {
        let slice = std::slice::from_raw_parts(value.as_ptr(), length as usize);
        let result = String::from_utf16_lossy(slice);
        // Free the memory allocated by the Windows API.
        CoTaskMemFree(Some(value.as_ptr() as *const std::ffi::c_void));
        result
    } else {
        String::new()
    }
}

/// Opens a camera device by its ID and applies the given configuration.
pub fn open(id: &str, config: CameraConfig) -> Result<(Box<dyn Stream>, DeviceControls)> {
    initialize_mf()?;

    let source_reader = unsafe { create_source_reader(id)? };
    let negotiated_fmt = negotiate_format(&source_reader, &config)?;
    unsafe { set_output_media_type(&source_reader, &negotiated_fmt)? };

    tracing::info!(
        "Camera opened: {}x{} @ {:?}",
        negotiated_fmt.width,
        negotiated_fmt.height,
        negotiated_fmt.format
    );

    let source_reader_arc = Arc::new(source_reader);
    let stream = MsmfStream::new(
        source_reader_arc.clone(),
        &negotiated_fmt,
        config.buffer_count,
    )?;
    let controls = create_controls(source_reader_arc);

    Ok((Box::new(stream), controls))
}

/// Creates and configures an `IMFSourceReader` for the camera device with the given ID.
unsafe fn create_source_reader(id: &str) -> Result<IMFSourceReader> {
    let mut attributes = None;
    MFCreateAttributes(&mut attributes, 2).map_err(hresult_to_camera_error)?;
    let attributes = attributes
        .ok_or_else(|| CameraError::Io(std::io::Error::other("Failed to create attributes")))?;

    attributes
        .SetGUID(
            &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
            &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
        )
        .map_err(hresult_to_camera_error)?;

    let id_hstring = HSTRING::from(id);
    attributes
        .SetString(
            &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
            &id_hstring,
        )
        .map_err(hresult_to_camera_error)?;

    let source = MFCreateDeviceSource(&attributes).map_err(hresult_to_camera_error)?;
    let source_reader =
        MFCreateSourceReaderFromMediaSource(&source, None).map_err(hresult_to_camera_error)?;

    select_video_stream(&source_reader)?;

    Ok(source_reader)
}

/// Selects only the first video stream from the source reader.
unsafe fn select_video_stream(source_reader: &IMFSourceReader) -> Result<()> {
    source_reader
        .SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)
        .map_err(hresult_to_camera_error)?;
    source_reader
        .SetStreamSelection(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, true)
        .map_err(hresult_to_camera_error)?;
    Ok(())
}

/// Sets the output media type on the source reader based on the negotiated format.
unsafe fn set_output_media_type(
    source_reader: &IMFSourceReader,
    negotiated_fmt: &NegotiatedFormat,
) -> Result<()> {
    let media_type = create_media_type(negotiated_fmt)?;

    source_reader
        .SetCurrentMediaType(
            MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32,
            None,
            &media_type,
        )
        .map_err(hresult_to_camera_error)?;

    source_reader
        .Flush(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32)
        .map_err(hresult_to_camera_error)?;

    Ok(())
}

unsafe fn create_media_type(format: &NegotiatedFormat) -> Result<IMFMediaType> {
    let media_type = MFCreateMediaType().map_err(hresult_to_camera_error)?;

    media_type
        .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
        .map_err(hresult_to_camera_error)?;

    let mf_guid = pixel_map::to_mf_guid(format.format).ok_or(CameraError::FormatNotSupported)?;
    media_type
        .SetGUID(&MF_MT_SUBTYPE, &mf_guid)
        .map_err(hresult_to_camera_error)?;

    media_type
        .SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
        .map_err(hresult_to_camera_error)?;

    let frame_size = ((format.width as u64) << 32) | (format.height as u64);
    media_type
        .SetUINT64(&MF_MT_FRAME_SIZE, frame_size)
        .map_err(hresult_to_camera_error)?;

    Ok(media_type)
}

/// Represents a video format that has been successfully negotiated with the device.
pub struct NegotiatedFormat {
    pub width: u32,
    pub height: u32,
    pub format: PixelFormat,
    pub fps: u32,
}

/// Iterates through the device's available formats and selects the best one based on the user's config.
fn negotiate_format(
    source_reader: &IMFSourceReader,
    config: &CameraConfig,
) -> Result<NegotiatedFormat> {
    (0..)
        .map(|index| unsafe {
            source_reader.GetNativeMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, index)
        })
        .take_while(|result| result.is_ok())
        .filter_map(|result| result.ok())
        .filter_map(|media_type| unsafe { parse_media_type(&media_type, config) })
        .max_by_key(|(score, _)| *score)
        .map(|(_, fmt)| fmt)
        .ok_or(CameraError::FormatNotSupported)
}

/// Parses a Media Foundation media type into a NegotiatedFormat.
///
/// This function extracts format information from an IMFMediaType object
/// and evaluates it against the user's configuration preferences.
///
/// # Arguments
///
/// * `media_type` - The Media Foundation media type to parse.
/// * `config` - The camera configuration containing format preferences.
///
/// # Returns
///
/// * `Some((score, format))` - The calculated score and negotiated format if parsing succeeds.
/// * `None` - If the media type is not a video type or parsing fails.
///
/// # Safety
///
/// This function is unsafe as it calls Windows Media Foundation APIs
/// that require unsafe context.
unsafe fn parse_media_type(
    media_type: &IMFMediaType,
    config: &CameraConfig,
) -> Option<(i32, NegotiatedFormat)> {
    if media_type.GetGUID(&MF_MT_MAJOR_TYPE).ok()? != MFMediaType_Video {
        return None;
    }

    let subtype = media_type.GetGUID(&MF_MT_SUBTYPE).ok()?;
    let core_fmt = pixel_map::from_mf_guid(&subtype);

    let frame_size = media_type.GetUINT64(&MF_MT_FRAME_SIZE).ok()?;
    let height = (frame_size & 0xFFFFFFFF) as u32;
    let width = ((frame_size >> 32) & 0xFFFFFFFF) as u32;

    let score = calculate_format_score(config, width, height, core_fmt);

    Some((
        score,
        NegotiatedFormat {
            width,
            height,
            format: core_fmt,
            fps: 30,
        },
    ))
}

/// Calculates a score for a given format based on the user's configuration preferences.
/// Higher scores indicate better matches. The score considers:
/// - Exact resolution matches (with priority weighting)
/// - Exact format matches (with priority weighting)
/// - Resolution distance (penalty for non-matching resolutions)
fn calculate_format_score(config: &CameraConfig, w: u32, h: u32, fmt: PixelFormat) -> i32 {
    // Score for exact resolution match, weighted by priority
    let resolution_score = config
        .resolution_req
        .iter()
        .find(|(req_w, req_h, _)| w == *req_w && h == *req_h)
        .map_or(0, |(_, _, prio)| *prio as i32 * 10);

    // Score for exact format match, weighted by priority
    let format_score = config
        .format_req
        .iter()
        .find(|(req_fmt, _)| fmt == *req_fmt)
        .map_or(0, |(_, prio)| *prio as i32 * 10);

    // Calculate distance from preferred resolutions if no exact match
    // This penalizes formats that are far from the requested resolution
    let resolution_distance = if resolution_score == 0 {
        config
            .resolution_req
            .iter()
            .map(|(req_w, req_h, _)| {
                let w_diff = (w as i32 - *req_w as i32).abs();
                let h_diff = (h as i32 - *req_h as i32).abs();
                -(w_diff + h_diff)
            })
            .max()
            .unwrap_or(-1000)
    } else {
        0
    };

    resolution_score + format_score + resolution_distance
}

/// Initializes the Media Foundation and COM subsystems with reference counting.
/// This function uses atomic operations to ensure thread-safe initialization.
/// The reference counting allows multiple camera instances to share the same MF/COM initialization.
pub fn initialize_mf() -> Result<()> {
    let initialized = &*INITIALIZED;
    let refcnt = &*CAMERA_REFCNT;

    if !initialized.load(Ordering::SeqCst) {
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
            .ok()
            .map_err(|e| CameraError::Io(std::io::Error::other(e.to_string())))?;

        unsafe { MFStartup(MF_API_VERSION, MFSTARTUP_NOSOCKET) }.map_err(|e| {
            unsafe {
                CoUninitialize();
            }
            CameraError::Io(std::io::Error::other(e.to_string()))
        })?;

        initialized.store(true, Ordering::SeqCst);
    }
    refcnt.fetch_add(1, Ordering::SeqCst);
    Ok(())
}

/// Shuts down the Media Foundation and COM subsystems using reference counting.
/// Only performs actual shutdown when the reference count reaches zero.
/// This ensures proper cleanup when all camera instances are closed.
pub fn shutdown_mf() {
    let refcnt = &*CAMERA_REFCNT;
    if refcnt.fetch_sub(1, Ordering::SeqCst) == 1 {
        let initialized = &*INITIALIZED;
        if initialized.load(Ordering::SeqCst) {
            unsafe {
                let _ = MFShutdown();
                CoUninitialize();
            }
            initialized.store(false, Ordering::SeqCst);
        }
    }
}
