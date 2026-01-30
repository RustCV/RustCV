use crate::core::mat::Mat;
use anyhow::{anyhow, Result};
use minifb::{Key, KeyRepeat, Window, WindowOptions};
use once_cell::sync::Lazy; // 我们在 Cargo.toml 里引入了这个库
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

// --- 全局窗口管理器 ---
// 使用 Lazy + Mutex 实现线程安全的全局状态
// 这让我们能像 OpenCV 一样通过字符串名称查找窗口
static WINDOW_MANAGER: Lazy<Mutex<HashMap<String, Window>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// 在指定窗口中显示图像
///
/// 这会完成以下工作：
/// 1. 如果窗口不存在，自动创建。
/// 2. 将 Mat (BGR/u8) 转换为 Minifb Buffer (ARGB/u32)。
/// 3. 刷新窗口内容。
pub fn imshow(winname: &str, mat: &Mat) -> Result<()> {
    // 1. 格式转换 (BGR u8 -> ARGB u32)
    // 这是 heavy lifting 的部分，虽然涉及拷贝，但为了跨平台显示是必须的。
    let buffer = mat_to_u32_buffer(mat)?;

    // 2. 获取全局锁
    let mut manager = WINDOW_MANAGER
        .lock()
        .map_err(|_| anyhow!("Failed to lock window manager"))?;

    // 3. 查找或创建窗口
    if let Some(window) = manager.get_mut(winname) {
        // 更新现有窗口
        // 注意：如果 mat 尺寸变了，minifb 会自动处理或报错，这里建议保持尺寸一致
        window
            .update_with_buffer(&buffer, mat.cols as usize, mat.rows as usize)
            .map_err(|e| anyhow!("Window update failed: {}", e))?;
    } else {
        // 创建新窗口
        let mut window = Window::new(
            winname,
            mat.cols as usize,
            mat.rows as usize,
            WindowOptions {
                resize: true, // 允许调整大小
                ..WindowOptions::default()
            },
        )
        .map_err(|e| anyhow!("Failed to create window: {}", e))?;

        // 初始更新
        window
            .update_with_buffer(&buffer, mat.cols as usize, mat.rows as usize)
            .map_err(|e| anyhow!("Initial window update failed: {}", e))?;

        manager.insert(winname.to_string(), window);
    }

    Ok(())
}

/// 等待按键 (简易版)
///
/// # 参数
/// * `delay`: 等待时间 (毫秒)。
///   - `0`: (在 minifb 中很难实现真正的无限等待且不阻塞消息循环，这里暂定为只刷新一次)
///   - `>0`: 睡眠指定时间并检测按键。
///
/// # 返回值
/// 返回按下的键的 ASCII 码 (如果有)，否则返回 -1 (类似 OpenCV)。
///
/// 注意：minifb 需要频繁调用 update 来响应 OS 消息。
/// 在这个实现中，imshow 负责 update 画面，wait_key 负责 update 输入状态。
pub fn wait_key(delay: i32) -> Result<i32> {
    // 获取锁来访问窗口状态
    let mut manager = WINDOW_MANAGER
        .lock()
        .map_err(|_| anyhow!("Failed to lock window manager"))?;

    // 简单的延时实现
    if delay > 0 {
        std::thread::sleep(Duration::from_millis(delay as u64));
    }

    // 遍历所有窗口，检查按键
    // 这是一个简化逻辑：我们只返回第一个被按下的键
    // 真正的 OpenCV waitKey 会处理所有窗口的 Event Loop
    for window in manager.values_mut() {
        // minifb 的 update 通常在显示时调用，但如果我们要捕获输入，
        // 必须确保窗口是活跃的。imshow 已经调用了 update_with_buffer。
        // 这里我们主要检查 Input。

        // 映射常用键
        if window.is_key_down(Key::Escape) {
            return Ok(27);
        } // ESC
        if window.is_key_down(Key::Space) {
            return Ok(32);
        } // Space
        if window.is_key_down(Key::Enter) {
            return Ok(13);
        } // Enter
        if window.is_key_down(Key::Q) {
            return Ok(113);
        } // q

        // TODO: 映射更多 minifb Key 到 ASCII
    }

    Ok(-1)
}

/// 销毁所有窗口
pub fn destroy_all_windows() -> Result<()> {
    let mut manager = WINDOW_MANAGER
        .lock()
        .map_err(|_| anyhow!("Failed to lock manager"))?;
    manager.clear(); // Drop Window 实例会自动关闭窗口
    Ok(())
}

// --- 内部辅助函数 ---

/// 将 BGR/RGB Mat 转换为 Minifb 需要的 ARGB u32 buffer
fn mat_to_u32_buffer(mat: &Mat) -> Result<Vec<u32>> {
    let pixel_count = (mat.rows * mat.cols) as usize;
    let mut buffer = Vec::with_capacity(pixel_count);

    // 假设 Mat 是 BGR 格式 (OpenCV 默认)
    // 且是 Packed (连续) 或 Strided
    // Minifb 格式: 00RR GGBB (0x00RRGGBB)

    let channels = mat.channels as usize;
    if channels != 3 {
        return Err(anyhow!("Currently only supports 3-channel (BGR) images"));
    }

    // 遍历每一行
    for r in 0..mat.rows {
        let row_data = mat.row_bytes(r); // 使用 Mat 的 row_bytes 处理 stride

        for c in 0..mat.cols as usize {
            let pixel_offset = c * channels;
            // 安全检查
            if pixel_offset + 2 >= row_data.len() {
                continue;
            }

            let b = row_data[pixel_offset] as u32;
            let g = row_data[pixel_offset + 1] as u32;
            let r = row_data[pixel_offset + 2] as u32;

            // Pack: 00 | R | G | B
            let pixel = (r << 16) | (g << 8) | b;
            buffer.push(pixel);
        }
    }

    Ok(buffer)
}
