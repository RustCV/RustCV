/// C interface to the AVFoundation camera capture bridge.
/// Rust's avf.rs calls these functions via FFI.
///
/// Thread safety:
///   avf_camera_open / start / stop / free — call from any single thread.
///   avf_camera_dequeue_blocking          — call from one consumer thread only.
///   AVFoundation delivers frames on its own internal capture queue.

#pragma once
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/// Opaque handle to a camera capture session.
typedef struct AvfCameraOpaque AvfCameraOpaque;

/// Return codes.
#define AVF_OK                   0
#define AVF_ERR_PERMISSION      (-1)
#define AVF_ERR_DEVICE_NOT_FOUND (-2)
#define AVF_ERR_SESSION         (-3)
#define AVF_ERR_STOPPED         (-4)

/// Open a camera by index and begin setup.
///
/// @param index        Device index (0 = first camera).
/// @param width        Requested frame width in pixels.
/// @param height       Requested frame height in pixels.
/// @param fps          Requested frames per second.
/// @param actual_width  Actual negotiated width (output).
/// @param actual_height Actual negotiated height (output).
/// @param out_cam      Receives the opaque camera handle on success.
/// @return AVF_OK on success, or a negative AVF_ERR_* code.
int avf_camera_open(uint32_t index,
                    uint32_t width, uint32_t height, uint32_t fps,
                    uint32_t *actual_width, uint32_t *actual_height,
                    AvfCameraOpaque **out_cam);

/// Start the capture session (begin delivering frames).
void avf_camera_start(AvfCameraOpaque *cam);

/// Stop the capture session and wake any blocked dequeue call.
void avf_camera_stop(AvfCameraOpaque *cam);

/// Block until the next frame is available, then copy it into `buf`.
///
/// @param cam       Camera handle.
/// @param buf       Caller-allocated destination buffer (BGRA32 pixels).
/// @param buf_cap   Capacity of `buf` in bytes.
/// @param out_len   Actual bytes written (output).
/// @param out_width Actual frame width (output).
/// @param out_height Actual frame height (output).
/// @param out_seq   Monotonic frame counter (output).
/// @param out_ts_us Presentation timestamp in microseconds (output).
/// @return AVF_OK on success, AVF_ERR_STOPPED if the session was stopped.
int avf_camera_dequeue_blocking(AvfCameraOpaque *cam,
                                uint8_t *buf, uint32_t buf_cap,
                                uint32_t *out_len,
                                uint32_t *out_width, uint32_t *out_height,
                                uint64_t *out_seq, uint64_t *out_ts_us);

/// Release all resources associated with the camera handle.
void avf_camera_free(AvfCameraOpaque *cam);

#ifdef __cplusplus
}
#endif
