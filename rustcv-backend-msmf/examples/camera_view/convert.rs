use rustcv_core::pixel_format::{FourCC, PixelFormat};

pub fn yuyv_to_rgb32(src: &[u8], dest: &mut [u32], width: usize, height: usize, stride: usize) {
    let expected_src_len = stride * height;
    let expected_dest_len = width * height;

    if src.len() < expected_src_len || dest.len() < expected_dest_len {
        return;
    }

    for row in 0..height {
        let row_src_start = row * stride;
        let row_dest_start = row * width;

        let mut src_idx = row_src_start;
        let mut dest_idx = row_dest_start;
        let row_end = row_src_start + width * 2;

        while src_idx + 3 < row_end && dest_idx + 1 < dest.len() {
            let y0 = src[src_idx] as i32;
            let u = src[src_idx + 1] as i32 - 128;
            let y1 = src[src_idx + 2] as i32;
            let v = src[src_idx + 3] as i32 - 128;

            let r0 = (298 * (y0 - 16) + 409 * v + 128) >> 8;
            let g0 = (298 * (y0 - 16) - 100 * u - 208 * v + 128) >> 8;
            let b0 = (298 * (y0 - 16) + 516 * u + 128) >> 8;

            let r1 = (298 * (y1 - 16) + 409 * v + 128) >> 8;
            let g1 = (298 * (y1 - 16) - 100 * u - 208 * v + 128) >> 8;
            let b1 = (298 * (y1 - 16) + 516 * u + 128) >> 8;

            dest[dest_idx] = rgb_to_u32(clamp(r0), clamp(g0), clamp(b0));
            if dest_idx + 1 < dest.len() {
                dest[dest_idx + 1] = rgb_to_u32(clamp(r1), clamp(g1), clamp(b1));
            }

            src_idx += 4;
            dest_idx += 2;
        }
    }
}

pub fn nv12_to_rgb32(src: &[u8], dest: &mut [u32], width: usize, height: usize, stride: usize) {
    let y_stride = stride;
    let uv_stride = stride;

    let y_plane_size = y_stride * height;
    let uv_height = height.div_ceil(2);
    let uv_plane_size = uv_stride * uv_height;
    let expected_src_len = y_plane_size + uv_plane_size;
    let expected_dest_len = width * height;

    if src.len() < expected_src_len || dest.len() < expected_dest_len {
        return;
    }

    let y_plane = &src[0..y_plane_size];
    let uv_plane = &src[y_plane_size..];

    for row in 0..height {
        let y_row_start = row * y_stride;
        let uv_row = row / 2;
        let uv_row_start = uv_row * uv_stride;

        for col in 0..width {
            let y_idx = y_row_start + col;
            let y = y_plane[y_idx] as i32;

            let uv_col = col / 2;
            let uv_idx = uv_row_start + uv_col * 2;

            let u = uv_plane[uv_idx] as i32 - 128;
            let v = uv_plane[uv_idx + 1] as i32 - 128;

            let r = (298 * (y - 16) + 409 * v + 128) >> 8;
            let g = (298 * (y - 16) - 100 * u - 208 * v + 128) >> 8;
            let b = (298 * (y - 16) + 516 * u + 128) >> 8;

            let dest_idx = row * width + col;
            dest[dest_idx] = rgb_to_u32(clamp(r), clamp(g), clamp(b));
        }
    }
}

#[inline]
fn clamp(val: i32) -> u32 {
    val.clamp(0, 255) as u32
}

#[inline]
fn rgb_to_u32(r: u32, g: u32, b: u32) -> u32 {
    (r << 16) | (g << 8) | b
}

pub fn is_format_supported(format: PixelFormat) -> bool {
    matches!(
        format,
        PixelFormat::Known(FourCC::YUYV) | PixelFormat::Known(FourCC::NV12)
    )
}
