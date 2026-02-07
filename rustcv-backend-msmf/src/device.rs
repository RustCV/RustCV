//! Device management for Windows Media Foundation (MSMF) camera backend.
//!
//! This module provides core functionality for:
//! - Media Foundation initialization and lifecycle management
//! - Camera device enumeration and discovery
//! - Format negotiation and selection
//! - Device stream creation and configuration

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

/// Flag tracking whether Media Foundation has been initialized
static INITIALIZED: LazyLock<Arc<AtomicBool>> = LazyLock::new(|| Arc::new(AtomicBool::new(false)));
/// Reference counter for Media Foundation instances
static CAMERA_REFCNT: LazyLock<Arc<AtomicUsize>> = LazyLock::new(|| Arc::new(AtomicUsize::new(0)));

/// Converts Windows HRESULT error to CameraError
///
/// # Arguments
/// * `e` - The HResult error from Windows API
///
/// # Returns
/// A CameraError wrapping the error message
fn hresult_to_camera_error(e: HResultError) -> CameraError {
    CameraError::Io(std::io::Error::other(e.to_string()))
}

/// Initializes the Media Foundation and COM subsystem
///
/// This function performs one-time initialization of the Windows Media Foundation
/// and COM runtime. It uses reference counting to ensure proper lifecycle management.
/// Multiple calls are safe; only the first call performs actual initialization.
///
/// Uses compare_exchange for efficient lock-free initialization.
pub fn initialize_mf() -> Result<()> {
    if INITIALIZED
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::Relaxed)
        .is_ok()
    {
        // We won the race to initialize
        unsafe {
            if let Err(e) = CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok() {
                INITIALIZED.store(false, Ordering::Release);
                return Err(CameraError::Io(std::io::Error::other(e.to_string())));
            }

            if let Err(e) = MFStartup(MF_API_VERSION, MFSTARTUP_NOSOCKET) {
                // Clean up COM if MF startup fails
                CoUninitialize();
                INITIALIZED.store(false, Ordering::Release);
                return Err(CameraError::Io(std::io::Error::other(e.to_string())));
            }
        }
    }
    // Increment reference count (use Release for synchronization with shutdown)
    CAMERA_REFCNT.fetch_add(1, Ordering::Release);
    Ok(())
}

/// Shuts down the Media Foundation subsystem
///
/// This function decrements the reference count and performs actual shutdown
/// when the count reaches zero. This ensures proper cleanup of resources.
/// Safe to call multiple times.
pub fn shutdown_mf() {
    let refcnt = &*CAMERA_REFCNT;
    // Decrement reference count with SeqCst to ensure proper synchronization
    // SeqCst ensures that all memory operations before this point are visible
    // to other threads before we check if we need to shutdown
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

/// Lists all available camera devices connected to the system
///
/// # Returns
/// A vector of `DeviceInfo` structs containing device name, ID, and backend information.
/// Returns an empty vector if no devices are found.
pub fn list_devices() -> Result<Vec<DeviceInfo>> {
    initialize_mf()?;

    unsafe {
        // Create media foundation attributes for device enumeration
        let mut attributes = MaybeUninit::<Option<IMFAttributes>>::uninit();
        MFCreateAttributes(attributes.as_mut_ptr(), 1).map_err(hresult_to_camera_error)?;
        let attributes = attributes
            .assume_init()
            .ok_or_else(|| CameraError::Io(std::io::Error::other("Failed to create attributes")))?;

        // Set the source type to video capture devices
        attributes
            .SetGUID(
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
            )
            .map_err(hresult_to_camera_error)?;

        // Enumerate all video capture devices
        let mut pp_devices = MaybeUninit::<*mut Option<IMFActivate>>::uninit();
        let mut count = MaybeUninit::<u32>::uninit();

        MFEnumDeviceSources(&attributes, pp_devices.as_mut_ptr(), count.as_mut_ptr())
            .map_err(hresult_to_camera_error)?;

        let pp_devices = pp_devices.assume_init();
        let count = count.assume_init();

        if count == 0 || pp_devices.is_null() {
            return Ok(Vec::new());
        }

        // Convert raw pointer to slice for safe iteration
        let slice = std::slice::from_raw_parts_mut(pp_devices, count as usize);
        let mut result = Vec::with_capacity(count as usize);

        // Extract device information from each IMFActivate object
        for activate_opt in slice {
            if let Some(device) = activate_opt.take() {
                let name = get_attribute_string(&device, MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME);

                // Skip devices with empty names early to avoid unnecessary ID retrieval
                if !name.is_empty() {
                    let id = get_attribute_string(
                        &device,
                        MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
                    );

                    result.push(DeviceInfo {
                        name,
                        id,
                        backend: "MSMF".to_string(),
                        bus_info: None,
                    });
                }
            }
        }

        // Free the allocated device array
        CoTaskMemFree(Some(pp_devices as *const _));
        Ok(result)
    }
}

/// Extracts a string attribute from a Media Foundation device
unsafe fn get_attribute_string(attr: &IMFActivate, guid: GUID) -> String {
    let mut value = MaybeUninit::uninit();
    let mut length = MaybeUninit::uninit();

    // Attempt to retrieve the string attribute.
    if attr
        .GetAllocatedString(&guid, value.as_mut_ptr(), length.as_mut_ptr())
        .is_err()
    {
        return String::new();
    }

    let value = value.assume_init();
    let length = length.assume_init();

    let result = {
        let slice = std::slice::from_raw_parts(value.as_ptr(), length as usize);
        String::from_utf16(slice).unwrap_or_else(|_| String::from_utf16_lossy(slice).to_owned())
    };

    // The memory was allocated by COM and must be freed with CoTaskMemFree.
    CoTaskMemFree(Some(value.as_ptr() as *const _));

    result
}

/// Opens a camera device and negotiates the video format based on configuration
///
/// This is the main entry point for opening a camera. The function:
/// 1. Initializes Media Foundation if not already initialized
/// 2. Creates a hardware MediaSource for the specified device
/// 3. Probes supported formats directly from the MediaSource
/// 4. Selects the best matching format based on configuration requirements
/// 5. Creates an async MsmfStream with proper format negotiation
/// 6. Sets up device control interfaces
///
/// # Arguments
/// * `id` - Device symbolic link ID or identifier string
/// * `config` - Camera configuration with resolution and format requirements
///
/// # Returns
/// A tuple containing:
/// - `Box<dyn Stream>` - The initialized camera stream for frame capture
/// - `DeviceControls` - Control interface for device-specific operations (exposure, focus, etc.)
pub fn open(id: &str, config: CameraConfig) -> Result<(Box<dyn Stream>, DeviceControls)> {
    initialize_mf()?;

    // 1. Find and create hardware MediaSource
    let media_source = unsafe { create_media_source(id)? };

    // 2. Directly probe supported formats from MediaSource without creating Reader yet
    let negotiated_fmt = negotiate_format(&media_source, &config)?;

    tracing::info!(
        "Camera opened: {}x{} @ {}fps ({:?})",
        negotiated_fmt.width,
        negotiated_fmt.height,
        negotiated_fmt.fps,
        negotiated_fmt.format
    );

    // 3. Create MsmfStream
    // Async Reader creation and callback binding are now fully encapsulated inside MsmfStream
    let stream = MsmfStream::new(&media_source, negotiated_fmt)
        .map_err(|e| CameraError::Io(std::io::Error::other(e.to_string())))?;

    let controls = create_controls(stream.get_reader());

    Ok((Box::new(stream), controls))
}

/// Creates a MediaSource for a specific camera device
///
/// # Arguments
/// * `id` - Device symbolic link ID
///
/// # Returns
/// The created IMFMediaSource interface for the device
unsafe fn create_media_source(id: &str) -> Result<IMFMediaSource> {
    let mut attributes = None;
    MFCreateAttributes(&mut attributes, 2).map_err(hresult_to_camera_error)?;
    let attributes = attributes.unwrap();

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

    MFCreateDeviceSource(&attributes).map_err(hresult_to_camera_error)
}

/// Negotiates and selects the best video format from available options
///
/// This function:
/// 1. Gets the presentation descriptor from the MediaSource
/// 2. Extracts the primary video stream descriptor
/// 3. Iterates through all available media types
/// 4. Parses each media type and calculates a match score
/// 5. Selects the format with the highest score
///
/// # Arguments
/// * `source` - The IMFMediaSource to query
/// * `config` - Camera configuration with format preferences
///
/// # Returns
/// A `NegotiatedFormat` struct with selected resolution, format, and frame rate
fn negotiate_format(source: &IMFMediaSource, config: &CameraConfig) -> Result<NegotiatedFormat> {
    unsafe {
        // Create presentation descriptor to access stream information
        let pd = source
            .CreatePresentationDescriptor()
            .map_err(hresult_to_camera_error)?;
        let mut selected = false.into();
        let mut sd = None;

        // Get the primary video stream descriptor (index 0)
        pd.GetStreamDescriptorByIndex(0, &mut selected, &mut sd)
            .map_err(hresult_to_camera_error)?;
        let sd =
            sd.ok_or_else(|| CameraError::Io(std::io::Error::other("No video stream found")))?;

        // Get media type handler for format enumeration
        let handler = sd.GetMediaTypeHandler().map_err(hresult_to_camera_error)?;
        let count = handler
            .GetMediaTypeCount()
            .map_err(hresult_to_camera_error)?;

        // Early exit if no media types available
        if count == 0 {
            return Err(CameraError::FormatNotSupported);
        }

        // Iterate through media types, tracking best format to avoid unnecessary clones
        let mut best_format: Option<(i32, NegotiatedFormat)> = None;

        for index in 0..count {
            if let Ok(mt) = handler.GetMediaTypeByIndex(index) {
                if let Some((score, fmt)) = parse_media_type(&mt, config) {
                    if best_format
                        .as_ref()
                        .is_none_or(|(best_score, _)| score > *best_score)
                    {
                        best_format = Some((score, fmt));
                    }
                }
            }
        }

        best_format
            .map(|(_, fmt)| fmt)
            .ok_or(CameraError::FormatNotSupported)
    }
}

/// Parses a Media Foundation media type and calculates format score
///
/// This function extracts video format parameters from an IMFMediaType and
/// calculates a score based on how well it matches the configuration requirements.
///
/// # Arguments
/// * `media_type` - The Media Foundation media type to parse
/// * `config` - Camera configuration with resolution and format preferences
///
/// # Returns
/// `Option<(i32, NegotiatedFormat)>` tuple containing:
/// - Score (higher is better)
/// - NegotiatedFormat with extracted parameters
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
    let width = ((frame_size >> 32) & 0xFFFFFFFF) as u32;
    let height = (frame_size & 0xFFFFFFFF) as u32;

    let fps = media_type
        .GetUINT64(&MF_MT_FRAME_RATE)
        .ok()
        .map(|val| (val >> 32) as u32 / (val & 0xFFFFFFFF).max(1) as u32)
        .unwrap_or(30);

    let score = calculate_format_score(config, width, height, core_fmt);

    Some((
        score,
        NegotiatedFormat {
            width,
            height,
            format: core_fmt,
            fps,
            media_type: media_type.clone(),
        },
    ))
}

/// Calculates a score for format selection based on configuration preferences
///
/// The scoring algorithm prioritizes:
/// 1. Exact resolution match (if configured)
/// 2. Supported pixel format (if configured)
/// 3. Closest resolution match (as fallback)
///
/// Uses a single-pass algorithm to avoid duplicate iterations of configuration vectors.
///
/// # Arguments
/// * `config` - Camera configuration with preferences and priorities
/// * `w` - Width of the format to score
/// * `h` - Height of the format to score
/// * `fmt` - Pixel format to score
///
/// # Returns
/// An integer score where higher values indicate better matches
fn calculate_format_score(config: &CameraConfig, w: u32, h: u32, fmt: PixelFormat) -> i32 {
    // Fast path: Check for exact resolution match while calculating distance in one pass
    let mut resolution_score = 0i32;
    let mut min_distance = i32::MAX;

    for (req_w, req_h, prio) in &config.resolution_req {
        if w == *req_w && h == *req_h {
            // Exact match found - highest priority
            resolution_score = *prio as i32 * 10;
            min_distance = 0; // Signal that exact match was found
            break;
        } else {
            // Track closest resolution during iteration
            let w_diff = (w as i32 - *req_w as i32).abs();
            let h_diff = (h as i32 - *req_h as i32).abs();
            let distance = w_diff + h_diff;
            if distance < min_distance {
                min_distance = distance;
            }
        }
    }

    // Score for supported format (multiplied by 10 for emphasis)
    let format_score = config
        .format_req
        .iter()
        .find(|(req_fmt, _)| fmt == *req_fmt)
        .map_or(0, |(_, prio)| *prio as i32 * 10);

    // Resolution distance penalty for non-exact matches
    let resolution_distance = if resolution_score > 0 {
        // Exact match found - no distance penalty
        0
    } else if min_distance < i32::MAX {
        // Approximate match - penalize by distance
        -min_distance
    } else if !config.resolution_req.is_empty() {
        // Required formats specified but none found - large penalty
        -1000
    } else {
        // No resolution requirements specified
        0
    };

    // Combined score: exact matches have priority, then format compatibility,
    // then closest resolution match
    resolution_score + format_score + resolution_distance
}

/// Represents a negotiated video format between application requirements and device capabilities.
///
/// This struct is produced by the format negotiation process and contains:
/// - The selected resolution (width × height)
/// - The pixel format (e.g., YUYV, NV12)
/// - The frame rate
/// - The Media Foundation media type for configuring the source reader
pub struct NegotiatedFormat {
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Pixel format (e.g., YUYV, NV12).
    pub format: PixelFormat,
    /// Frame rate in frames per second.
    #[allow(dead_code)]
    pub fps: u32,
    /// The Media Foundation media type for the source reader.
    pub media_type: IMFMediaType,
}
