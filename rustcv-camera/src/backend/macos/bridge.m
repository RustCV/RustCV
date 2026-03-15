/// AVFoundation camera capture bridge — Objective-C implementation.
///
/// Design overview:
///
///   ┌─────────────────────────────────────────────────────────────────┐
///   │  AVFoundation (system)                                          │
///   │    AVCaptureSession → AVCaptureVideoDataOutput                  │
///   │              ↓ (serial capture queue)                           │
///   │    AvfDelegate.captureOutput:...                                │
///   │      - CVPixelBuffer lock (BGRA32, hardware native resolution)  │
///   │      - if hw_size != target_size:                               │
///   │          vImageScale_ARGB8888 → cam->frame_data (SIMD scale)   │
///   │        else:                                                     │
///   │          memcpy → cam->frame_data                              │
///   │      - pthread_cond_signal                                      │
///   └─────────────────────────────────────────────────────────────────┘
///           ↑ pthread_mutex + pthread_cond
///   ┌───────────────────────────────────────────────┐
///   │  Rust thread (avf_camera_dequeue_blocking)    │
///   │    - pthread_cond_wait until has_frame        │
///   │    - memcpy frame_data → caller's buffer      │
///   │    - return AVF_OK                            │
///   └───────────────────────────────────────────────┘
///
/// Pixel format: kCVPixelFormatType_32BGRA (4 bytes per pixel).
/// This matches PixelFormat::Bgra32 on the Rust side.
///
/// Resolution contract:
/// The bridge always delivers frames at exactly (target_w × target_h).
/// If the camera hardware outputs at a different native resolution
/// (e.g. minimum 1280×720 on modern MacBooks), vImageScale_ARGB8888
/// from Accelerate.framework performs SIMD-accelerated scaling before
/// signalling the condvar.  The caller therefore always receives the
/// exact dimensions requested via avf_camera_open().

@import AVFoundation;
@import Accelerate;
@import CoreMedia;
@import CoreVideo;
@import Foundation;

#include <pthread.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <limits.h>
#include "bridge.h"

// ─── AvfCameraOpaque ─────────────────────────────────────────────────────────

struct AvfCameraOpaque {
    // AVFoundation objects, retained with CFBridgingRetain.
    CFTypeRef session;       // AVCaptureSession *
    CFTypeRef delegate_obj;  // AvfDelegate *

    // Frame buffer — written by the delegate, read by dequeue_blocking.
    // Always contains a frame at (target_w × target_h) BGRA32 after scaling.
    uint8_t  *frame_data;
    uint32_t  frame_data_len;
    uint32_t  frame_data_cap;

    // User-requested output dimensions.  The delegate scales every hardware
    // frame to exactly these dimensions before signalling has_frame = 1.
    uint32_t  target_w;
    uint32_t  target_h;

    // Frame metadata (reflects post-scale dimensions).
    uint32_t  width;
    uint32_t  height;
    uint64_t  sequence;
    uint64_t  timestamp_us;

    // Synchronisation between the capture callback and dequeue_blocking.
    pthread_mutex_t mutex;
    pthread_cond_t  cond;
    int has_frame;  // 1 when a new frame is ready to consume
    int stopped;    // 1 after avf_camera_stop() is called
};

// ─── AvfDelegate ─────────────────────────────────────────────────────────────

@interface AvfDelegate : NSObject <AVCaptureVideoDataOutputSampleBufferDelegate>
@property (nonatomic, assign) struct AvfCameraOpaque *cam;
@end

@implementation AvfDelegate

- (void)captureOutput:(AVCaptureOutput *)output
didOutputSampleBuffer:(CMSampleBufferRef)sampleBuffer
       fromConnection:(AVCaptureConnection *)connection
{
    struct AvfCameraOpaque *cam = self.cam;
    if (!cam || cam->stopped) return;

    CVImageBufferRef imgBuf = CMSampleBufferGetImageBuffer(sampleBuffer);
    if (!imgBuf) return;

    CVPixelBufferLockBaseAddress(imgBuf, kCVPixelBufferLock_ReadOnly);

    void    *base      = CVPixelBufferGetBaseAddress(imgBuf);
    uint32_t hw_w      = (uint32_t)CVPixelBufferGetWidth(imgBuf);
    uint32_t hw_h      = (uint32_t)CVPixelBufferGetHeight(imgBuf);
    size_t   row_bytes = CVPixelBufferGetBytesPerRow(imgBuf); // may be padded

    CMTime pts    = CMSampleBufferGetPresentationTimeStamp(sampleBuffer);
    uint64_t ts   = (uint64_t)(CMTimeGetSeconds(pts) * 1000000.0);

    pthread_mutex_lock(&cam->mutex);

    uint32_t out_w   = cam->target_w;
    uint32_t out_h   = cam->target_h;
    uint32_t out_cap = out_w * out_h * 4;

    // Grow the output buffer if necessary (e.g. first frame or config change).
    if (out_cap > cam->frame_data_cap) {
        cam->frame_data     = (uint8_t *)realloc(cam->frame_data, out_cap);
        cam->frame_data_cap = out_cap;
    }

    if (hw_w != out_w || hw_h != out_h) {
        // Hardware resolution differs from the requested resolution.
        // Use vImageScale_ARGB8888 (Accelerate.framework) for SIMD-accelerated
        // scaling.  Works for both upscale and downscale.
        //
        // Note: vImage treats ARGB8888 and BGRA8888 identically — it scales
        // each of the 4 channels independently, so the channel order does not
        // matter here.
        vImage_Buffer src = {
            .data     = base,
            .height   = hw_h,
            .width    = hw_w,
            .rowBytes = row_bytes,   // actual stride, may include row padding
        };
        vImage_Buffer dst = {
            .data     = cam->frame_data,
            .height   = out_h,
            .width    = out_w,
            .rowBytes = (size_t)out_w * 4,  // packed output, no padding
        };
        vImageScale_ARGB8888(&src, &dst, NULL, kvImageNoFlags);
    } else {
        // Dimensions already match — direct copy is faster than vImage.
        // Use row_bytes * hw_h rather than CVPixelBufferGetDataSize to avoid
        // including any tail padding that GetDataSize may report.
        memcpy(cam->frame_data, base, (size_t)row_bytes * hw_h);
    }

    cam->frame_data_len = out_cap;
    cam->width          = out_w;
    cam->height         = out_h;
    cam->sequence      += 1;
    cam->timestamp_us   = ts;
    cam->has_frame      = 1;

    pthread_cond_signal(&cam->cond);
    pthread_mutex_unlock(&cam->mutex);

    CVPixelBufferUnlockBaseAddress(imgBuf, kCVPixelBufferLock_ReadOnly);
}

@end

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Request camera permission synchronously (blocks until the user responds).
/// Returns 1 if authorized, 0 otherwise.
static int request_camera_permission(void)
{
    AVAuthorizationStatus status =
        [AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeVideo];

    if (status == AVAuthorizationStatusNotDetermined) {
        dispatch_semaphore_t sem = dispatch_semaphore_create(0);
        [AVCaptureDevice requestAccessForMediaType:AVMediaTypeVideo
                               completionHandler:^(BOOL granted) {
            (void)granted;
            dispatch_semaphore_signal(sem);
        }];
        dispatch_semaphore_wait(sem, DISPATCH_TIME_FOREVER);
        status = [AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeVideo];
    }

    return (status == AVAuthorizationStatusAuthorized) ? 1 : 0;
}

/// Enumerate all video capture devices (built-in and external).
static NSArray<AVCaptureDevice *> *enumerate_video_devices(void)
{
    // Use the deprecated-but-universal API to avoid SDK version #ifdefs.
    // This works on macOS 10.7–14+ and produces a deprecation warning on 10.15+
    // which we suppress here.
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
    return [AVCaptureDevice devicesWithMediaType:AVMediaTypeVideo];
#pragma clang diagnostic pop
}

// ─── C API ───────────────────────────────────────────────────────────────────

int avf_camera_open(uint32_t index,
                    uint32_t width, uint32_t height, uint32_t fps,
                    uint32_t *actual_width, uint32_t *actual_height,
                    struct AvfCameraOpaque **out_cam)
{
    // 1. Check / request camera permission.
    if (!request_camera_permission()) {
        return AVF_ERR_PERMISSION;
    }

    // 2. Find the requested device.
    NSArray<AVCaptureDevice *> *devices = enumerate_video_devices();
    if (index >= (uint32_t)devices.count) {
        return AVF_ERR_DEVICE_NOT_FOUND;
    }
    AVCaptureDevice *device = devices[index];

    // 3. Create the capture session.
    AVCaptureSession *session = [[AVCaptureSession alloc] init];

    // 4. Add the device as an input.
    NSError *error = nil;
    AVCaptureDeviceInput *inputDev =
        [AVCaptureDeviceInput deviceInputWithDevice:device error:&error];
    if (!inputDev || ![session canAddInput:inputDev]) {
        return AVF_ERR_SESSION;
    }
    [session addInput:inputDev];

    // 5. Choose the session preset closest to the requested resolution.
    //    AVCaptureSessionPreset reliably controls BGRA32 output dimensions,
    //    unlike device.activeFormat which reports compressed-format nominal sizes.
    //
    //    Preset table (width × height):
    //      352×288, 640×480, 1280×720, 1920×1080
    //    We pick the one whose long-edge distance is smallest.
    struct { NSString * __unsafe_unretained preset; uint32_t w; uint32_t h; } kPresets[] = {
        { AVCaptureSessionPreset352x288,  352,  288 },
        { AVCaptureSessionPreset640x480,  640,  480 },
        { AVCaptureSessionPreset1280x720,  1280,  720 },
        { AVCaptureSessionPreset1920x1080, 1920, 1080 },
    };

    NSString *chosenPreset = AVCaptureSessionPreset640x480;
    int32_t bestScore = INT32_MAX;

    for (size_t i = 0; i < sizeof(kPresets)/sizeof(kPresets[0]); i++) {
        if (![session canSetSessionPreset:kPresets[i].preset]) continue;
        int32_t dw = abs((int32_t)kPresets[i].w - (int32_t)width);
        int32_t dh = abs((int32_t)kPresets[i].h - (int32_t)height);
        if (dw + dh < bestScore) {
            bestScore    = dw + dh;
            chosenPreset = kPresets[i].preset;
        }
    }
    session.sessionPreset = chosenPreset;

    // Set the frame rate on the device.
    if ([device lockForConfiguration:&error]) {
        CMTime duration = CMTimeMake(1, (int32_t)fps);
        for (AVFrameRateRange *range in device.activeFormat.videoSupportedFrameRateRanges) {
            if ((double)fps >= range.minFrameRate && (double)fps <= range.maxFrameRate) {
                device.activeVideoMinFrameDuration = duration;
                device.activeVideoMaxFrameDuration = duration;
                break;
            }
        }
        [device unlockForConfiguration];
    }

    // 6. Create the video output with BGRA32 pixel format.
    AVCaptureVideoDataOutput *videoOutput =
        [[AVCaptureVideoDataOutput alloc] init];
    videoOutput.videoSettings = @{
        (NSString *)kCVPixelBufferPixelFormatTypeKey:
            @(kCVPixelFormatType_32BGRA)
    };
    // Drop frames if the consumer (dequeue_blocking) is busy rather than
    // accumulating a queue of stale frames.
    videoOutput.alwaysDiscardsLateVideoFrames = YES;

    if (![session canAddOutput:videoOutput]) {
        return AVF_ERR_SESSION;
    }
    [session addOutput:videoOutput];

    // 7. Allocate the internal camera struct.
    struct AvfCameraOpaque *cam =
        (struct AvfCameraOpaque *)calloc(1, sizeof(struct AvfCameraOpaque));
    if (!cam) return AVF_ERR_SESSION;

    pthread_mutex_init(&cam->mutex, NULL);
    pthread_cond_init(&cam->cond, NULL);

    // Store the user-requested target dimensions.  The delegate will scale
    // every hardware frame to exactly these dimensions using vImage.
    cam->target_w = width;
    cam->target_h = height;
    cam->width    = width;
    cam->height   = height;

    // Pre-allocate the frame buffer for the TARGET resolution (post-scale).
    // The bridge always delivers (target_w × target_h) BGRA32 to the caller.
    uint32_t buf_size   = width * height * 4;
    cam->frame_data     = (uint8_t *)malloc(buf_size);
    cam->frame_data_cap = buf_size;

    // 8. Create the delegate and connect it to the output.
    AvfDelegate *delegate  = [[AvfDelegate alloc] init];
    delegate.cam           = cam;

    dispatch_queue_t queue =
        dispatch_queue_create("com.rustcv.camera.capture", DISPATCH_QUEUE_SERIAL);
    [videoOutput setSampleBufferDelegate:delegate queue:queue];

    // Retain ObjC objects so ARC doesn't release them when the local variables
    // go out of scope; we take manual ownership via CFBridgingRetain.
    cam->session      = CFBridgingRetain(session);
    cam->delegate_obj = CFBridgingRetain(delegate);

    // Report the output dimensions: the bridge guarantees delivery at
    // (width × height) after any necessary vImage scaling.
    *actual_width  = width;
    *actual_height = height;
    *out_cam       = cam;
    return AVF_OK;
}

void avf_camera_start(struct AvfCameraOpaque *cam)
{
    AVCaptureSession *session = (__bridge AVCaptureSession *)cam->session;
    [session startRunning];
}

void avf_camera_stop(struct AvfCameraOpaque *cam)
{
    AVCaptureSession *session = (__bridge AVCaptureSession *)cam->session;
    [session stopRunning];

    // Wake any thread blocked in avf_camera_dequeue_blocking.
    pthread_mutex_lock(&cam->mutex);
    cam->stopped = 1;
    pthread_cond_broadcast(&cam->cond);
    pthread_mutex_unlock(&cam->mutex);
}

int avf_camera_dequeue_blocking(struct AvfCameraOpaque *cam,
                                uint8_t *buf, uint32_t buf_cap,
                                uint32_t *out_len,
                                uint32_t *out_width, uint32_t *out_height,
                                uint64_t *out_seq, uint64_t *out_ts_us)
{
    pthread_mutex_lock(&cam->mutex);

    // Wait until a frame arrives or the session is stopped.
    while (!cam->has_frame && !cam->stopped) {
        pthread_cond_wait(&cam->cond, &cam->mutex);
    }

    if (cam->stopped && !cam->has_frame) {
        pthread_mutex_unlock(&cam->mutex);
        return AVF_ERR_STOPPED;
    }

    // Copy frame data into the caller's buffer.
    uint32_t copy_len =
        cam->frame_data_len < buf_cap ? cam->frame_data_len : buf_cap;
    memcpy(buf, cam->frame_data, copy_len);

    *out_len    = copy_len;
    *out_width  = cam->width;
    *out_height = cam->height;
    *out_seq    = cam->sequence;
    *out_ts_us  = cam->timestamp_us;
    cam->has_frame = 0;  // consumed

    pthread_mutex_unlock(&cam->mutex);
    return AVF_OK;
}

void avf_camera_free(struct AvfCameraOpaque *cam)
{
    if (!cam) return;

    // Release the retained ObjC objects.
    if (cam->session)      CFRelease(cam->session);
    if (cam->delegate_obj) CFRelease(cam->delegate_obj);

    free(cam->frame_data);
    pthread_mutex_destroy(&cam->mutex);
    pthread_cond_destroy(&cam->cond);
    free(cam);
}
