use crate::core::mat::Mat;
use anyhow::{anyhow, Result};
use minifb::{Key, Window, WindowOptions}; // 去掉了 KeyRepeat，根据版本可能不需要
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

// --- 关键修正：线程安全包装器 ---
// minifb::Window 在 Linux 上包含 X11 裸指针，默认不是 Send 的。
// 我们需要包装一下，并承诺它是 Send 的，以便存入全局 Mutex。
struct SendWindow(Window);

// ⚠️ UNSAFE: 强制标记为 Send。
// 前提：我们在主线程中使用 imshow，且 Mutex 保证了同一时间只有一个线程访问。
unsafe impl Send for SendWindow {}

// --- 全局窗口管理器 ---
// 使用 SendWindow 替代 Window
static WINDOW_MANAGER: Lazy<Mutex<HashMap<String, SendWindow>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// 在指定窗口中显示图像
pub fn imshow(winname: &str, mat: &Mat) -> Result<()> {
    // 1. 格式转换
    let buffer = mat_to_u32_buffer(mat)?;
    let width = mat.cols as usize;
    let height = mat.rows as usize;

    // 2. 获取全局锁
    let mut manager = WINDOW_MANAGER
        .lock()
        .map_err(|_| anyhow!("Failed to lock window manager"))?;

    // 3. 检查是否需要重建窗口
    let need_recreate = if let Some(wrapper) = manager.get(winname) {
        // 获取当前窗口的尺寸
        let (win_w, win_h) = wrapper.0.get_size();
        // 如果尺寸不匹配（说明分辨率变了），标记为需要重建
        // 注意：这里容忍一点点误差，或者严格匹配
        win_w != width || win_h != height
    } else {
        true // 窗口不存在，肯定要创建
    };

    if need_recreate {
        // 如果存在旧窗口，先移除（Drop 会自动关闭窗口）
        if manager.contains_key(winname) {
            manager.remove(winname);
        }

        // 创建新窗口
        let mut window = Window::new(
            winname,
            width,
            height,
            WindowOptions {
                resize: true, // 允许用户手动缩放
                ..WindowOptions::default()
            },
        )
        .map_err(|e| anyhow!("Failed to create window: {}", e))?;

        // 初始更新
        window
            .update_with_buffer(&buffer, width, height)
            .map_err(|e| anyhow!("Initial update failed: {}", e))?;

        // 存入管理器
        manager.insert(winname.to_string(), SendWindow(window));
    } else {
        // 窗口已存在且尺寸匹配，直接更新
        if let Some(wrapper) = manager.get_mut(winname) {
            wrapper
                .0
                .update_with_buffer(&buffer, width, height)
                .map_err(|e| anyhow!("Window update failed: {}", e))?;
        }
    }

    Ok(())
}

/// 等待按键
pub fn wait_key(delay: i32) -> Result<i32> {
    let mut manager = WINDOW_MANAGER
        .lock()
        .map_err(|_| anyhow!("Failed to lock window manager"))?;

    if delay > 0 {
        std::thread::sleep(Duration::from_millis(delay as u64));
    }

    for wrapper in manager.values_mut() {
        let window = &mut wrapper.0; // 访问内部 Window

        // 映射常用键
        if window.is_key_down(Key::Escape) {
            return Ok(27);
        }
        if window.is_key_down(Key::Space) {
            return Ok(32);
        }
        if window.is_key_down(Key::Enter) {
            return Ok(13);
        }
        if window.is_key_down(Key::Q) {
            return Ok(113);
        }
    }

    Ok(-1)
}

/// 销毁所有窗口
pub fn destroy_all_windows() -> Result<()> {
    let mut manager = WINDOW_MANAGER
        .lock()
        .map_err(|_| anyhow!("Failed to lock manager"))?;
    manager.clear();
    Ok(())
}

// --- 内部辅助函数 (保持不变) ---
fn mat_to_u32_buffer(mat: &Mat) -> Result<Vec<u32>> {
    let pixel_count = (mat.rows * mat.cols) as usize;
    let mut buffer = Vec::with_capacity(pixel_count);
    let channels = mat.channels as usize;

    if channels != 3 {
        return Err(anyhow!("Currently only supports 3-channel (BGR) images"));
    }

    for r in 0..mat.rows {
        let row_data = mat.row_bytes(r);

        for c in 0..mat.cols as usize {
            let pixel_offset = c * channels;
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
