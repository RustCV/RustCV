use std::time::{Duration, Instant};

/// 复刻 OpenCV C++ 的 cv::TickMeter 类
/// 这是一个高精度的秒表，专门用于计算 FPS 和耗时
pub struct TickMeter {
    start_time: Option<Instant>, // 按下秒表的时刻
    total_time: Duration,        // 累计走过的时间
    counter: u64,                // 计次（比如统计了多少帧）
}

impl Default for TickMeter {
    fn default() -> Self {
        Self::new()
    }
}

impl TickMeter {
    /// 构造函数
    pub fn new() -> Self {
        Self {
            start_time: None,
            total_time: Duration::new(0, 0),
            counter: 0,
        }
    }

    /// 开始计时 (Start)
    pub fn start(&mut self) {
        if self.start_time.is_none() {
            self.start_time = Some(Instant::now());
        }
    }

    /// 停止计时 (Stop)
    /// 停止后，时间会累积到 total_time 中，且计数器 +1
    pub fn stop(&mut self) {
        if let Some(start) = self.start_time {
            let elapsed = start.elapsed();
            self.total_time += elapsed;
            self.counter += 1;
            self.start_time = None; //以此标记为停止状态
        }
    }

    /// 重置秒表 (Reset)
    pub fn reset(&mut self) {
        self.start_time = None;
        self.total_time = Duration::new(0, 0);
        self.counter = 0;
    }

    /// 获取当前的 FPS (Frames Per Second)
    /// 公式：次数 / 总耗时
    pub fn get_fps(&self) -> f64 {
        let secs = self.total_time.as_secs_f64();
        if secs > 0.0 {
            self.counter as f64 / secs
        } else {
            0.0
        }
    }

    /// 获取当前累计的帧数
    pub fn get_counter(&self) -> u64 {
        self.counter
    }
}
