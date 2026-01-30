use crate::core::mat::Mat;
use anyhow::{anyhow, Result};
use std::path::Path;

/// 读取图像文件
///
/// 支持 JPG, PNG, BMP 等常见格式。
/// 注意：这将强制转换为 BGR 格式以匹配 OpenCV 默认行为。
pub fn imread<P: AsRef<Path>>(path: P) -> Result<Mat> {
    // 1. 使用 image crate 加载
    let img = image::open(path).map_err(|e| anyhow!("Failed to open image: {}", e))?;

    // 2. 转换为 RGB8 (统一格式)
    let rgb = img.to_rgb8();
    let (width, height) = (rgb.width() as i32, rgb.height() as i32);

    // 3. 转换为 BGR (OpenCV 默认)
    // 这是一个内存拷贝过程，但对于文件 IO 来说，性能瓶颈通常在磁盘读取，而不是这里的内存重排。
    let pixel_count = (width * height) as usize;
    let mut bgr_data = Vec::with_capacity(pixel_count * 3);

    for pixel in rgb.pixels() {
        let [r, g, b] = pixel.0;
        bgr_data.push(b); // Blue first
        bgr_data.push(g);
        bgr_data.push(r); // Red last
    }

    // 4. 构造 Mat (Packed layout, so step = width * 3)
    let mut mat = Mat::new(height, width, 3);
    mat.data = bgr_data;

    Ok(mat)
}

/// 保存图像文件
///
/// 根据文件扩展名自动决定格式。
pub fn imwrite<P: AsRef<Path>>(path: P, mat: &Mat) -> Result<()> {
    if mat.channels != 3 {
        return Err(anyhow!(
            "Only 3-channel (BGR) images are supported for saving currently"
        ));
    }

    // 1. BGR -> RGB 转换
    // image crate save 需要 RGB
    let pixel_count = (mat.rows * mat.cols) as usize;
    let mut rgb_data = Vec::with_capacity(pixel_count * 3);

    for r in 0..mat.rows {
        let row = mat.row_bytes(r);
        for c in 0..mat.cols as usize {
            let offset = c * 3;
            let b = row[offset];
            let g = row[offset + 1];
            let r = row[offset + 2];

            rgb_data.push(r);
            rgb_data.push(g);
            rgb_data.push(b);
        }
    }

    // 2. 保存
    image::save_buffer(
        path,
        &rgb_data,
        mat.cols as u32,
        mat.rows as u32,
        image::ColorType::Rgb8,
    )
    .map_err(|e| anyhow!("Failed to save image: {}", e))?;

    Ok(())
}
