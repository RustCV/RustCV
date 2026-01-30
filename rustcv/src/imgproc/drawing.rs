use crate::core::mat::Mat;
use rusttype::{point, Font, PositionedGlyph, Scale};
use std::sync::OnceLock;

// --- 基础结构 ---

#[derive(Clone, Copy, Debug)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Scalar {
    pub v0: u8, // Blue
    pub v1: u8, // Green
    pub v2: u8, // Red
}

impl Scalar {
    pub fn new(b: u8, g: u8, r: u8) -> Self {
        Self {
            v0: b,
            v1: g,
            v2: r,
        }
    }
    pub fn all(v: u8) -> Self {
        Self {
            v0: v,
            v1: v,
            v2: v,
        }
    }
}

// --- 绘图函数 ---

/// 在 Mat 上绘制矩形 (In-place)
///
/// 这是一个手动实现的高性能版本，直接操作 Vec<u8>，避免了任何类型转换。
pub fn rectangle(mat: &mut Mat, rect: Rect, color: Scalar, thickness: i32) {
    let x_min = rect.x.max(0);
    let y_min = rect.y.max(0);
    let x_max = (rect.x + rect.width).min(mat.cols);
    let y_max = (rect.y + rect.height).min(mat.rows);

    if x_min >= x_max || y_min >= y_max {
        return;
    }

    // 辅助闭包：设置像素
    // 注意：Rust 借用检查器可能不喜欢我们在循环里多次借用 mat.data，所以我们用 raw slice 或 index
    // 为了代码清晰，这里用 safe index，release 模式下会被优化
    let set_pixel = |data: &mut Vec<u8>, step: usize, r: i32, c: i32, color: Scalar| {
        let idx = (r as usize) * step + (c as usize) * 3;
        if idx + 2 < data.len() {
            data[idx] = color.v0;
            data[idx + 1] = color.v1;
            data[idx + 2] = color.v2;
        }
    };

    let step = mat.step;

    // 绘制上下边
    for c in x_min..x_max {
        for t in 0..thickness {
            set_pixel(&mut mat.data, step, y_min + t, c, color); // Top
            set_pixel(&mut mat.data, step, y_max - 1 - t, c, color); // Bottom
        }
    }

    // 绘制左右边
    for r in y_min..y_max {
        for t in 0..thickness {
            set_pixel(&mut mat.data, step, r, x_min + t, color); // Left
            set_pixel(&mut mat.data, step, r, x_max - 1 - t, color); // Right
        }
    }
}

// --- 文本渲染 ---

// 嵌入字体数据：为了开箱即用，我们尝试包含一个 assets 目录下的字体
// 如果编译时找不到文件，这里会报错。
// 实际工程中，建议使用 cfg 控制或运行时加载。
// 这里为了演示方便，我们假设 assets/DejaVuSans.ttf 存在。
// 如果你不想下载字体，可以把这个 static 改成 None，然后运行时报错提示。
static FONT_DATA: &[u8] = include_bytes!("../assets/font.ttf");
static FONT: OnceLock<Font> = OnceLock::new();

fn get_font() -> &'static Font<'static> {
    FONT.get_or_init(|| Font::try_from_bytes(FONT_DATA).expect("Error constructing Font"))
}

/// 在图像上绘制文字
pub fn put_text(mat: &mut Mat, text: &str, org: Point, font_scale: f32, color: Scalar) {
    let font = get_font();
    let scale = Scale::uniform(font_scale * 20.0); // 调整倍率以匹配 OpenCV 手感
    let start = point(org.x as f32, org.y as f32);
    let glyphs: Vec<PositionedGlyph> = font.layout(text, scale, start).collect();

    let step = mat.step;
    let rows = mat.rows;
    let cols = mat.cols;
    let channels = 3;

    for glyph in glyphs {
        if let Some(bounding_box) = glyph.pixel_bounding_box() {
            // 栅格化每个字符
            glyph.draw(|x, y, v| {
                // v 是覆盖率 (0.0 - 1.0)，用于抗锯齿混合
                let px = x as i32 + bounding_box.min.x;
                let py = y as i32 + bounding_box.min.y;

                if px >= 0 && px < cols && py >= 0 && py < rows {
                    let idx = (py as usize) * step + (px as usize) * channels;

                    // 简单的 Alpha Blending
                    // Current Pixel
                    let b_old = mat.data[idx] as f32;
                    let g_old = mat.data[idx + 1] as f32;
                    let r_old = mat.data[idx + 2] as f32;

                    let alpha = v;
                    let b_new = (color.v0 as f32 * alpha) + (b_old * (1.0 - alpha));
                    let g_new = (color.v1 as f32 * alpha) + (g_old * (1.0 - alpha));
                    let r_new = (color.v2 as f32 * alpha) + (r_old * (1.0 - alpha));

                    mat.data[idx] = b_new as u8;
                    mat.data[idx + 1] = g_new as u8;
                    mat.data[idx + 2] = r_new as u8;
                }
            });
        }
    }
}
