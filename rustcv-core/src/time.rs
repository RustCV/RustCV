use std::collections::VecDeque;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

// 1. 定义一个全局静态变量来存储进程启动时间
// OnceLock 保证它只会被初始化一次，且是线程安全的。
static PROCESS_START: OnceLock<Instant> = OnceLock::new();
static PROCESS_START_TIME: OnceLock<Instant> = OnceLock::new();

/// 软件锁相环 (Software PLL) 与时间同步器
///
/// 解决两个问题：
/// 1. 硬件时钟 (Hardware Timestamp) 通常与系统时钟 (System Time) 不同步。
/// 2. 硬件时钟存在漂移 (Drift)，且 USB 传输导致到达时间 (Arrival Time) 有抖动 (Jitter)。
///
/// 算法：基于最小二乘法的线性回归 (Linear Regression on Sliding Window)
#[derive(Debug)]
pub struct ClockSynchronizer {
    /// 滑动窗口大小 (例如最近 30 帧)
    window_size: usize,
    /// 历史数据点 (HW_Timestamp, System_Arrival_Time)
    history: VecDeque<(u64, Instant)>,
    /// 是否已初始化基准
    #[allow(dead_code)]
    baseline_established: bool,
    /// 估算的斜率 (Drift Rate)
    estimated_slope: f64,
    /// 估算的截距 (Offset)
    estimated_offset: f64,
}

impl ClockSynchronizer {
    pub fn new(window_size: usize) -> Self {
        Self {
            window_size: window_size.max(2), // 至少两点决定一条直线
            history: VecDeque::with_capacity(window_size),
            baseline_established: false,
            estimated_slope: 1.0,
            estimated_offset: 0.0,
        }
    }

    /// 输入一帧的原始硬件时间戳，返回矫正后的系统时间
    ///
    /// * `hw_ns`: 驱动提供的硬件时间戳 (纳秒)
    /// * `arrival_time`: 帧到达用户态的系统时刻
    pub fn correct(&mut self, hw_ns: u64, arrival_time: Instant) -> Duration {
        // 1. 记录数据点
        if self.history.len() >= self.window_size {
            self.history.pop_front();
        }
        self.history.push_back((hw_ns, arrival_time));

        // 2. 如果数据不够，直接返回到达时间作为降级方案
        if self.history.len() < 5 {
            // 在初始化阶段，假设无漂移，直接对齐到第一帧
            if let Some(start_time) = self.history.front() {
                // 简单偏移计算
                let elapsed_hw = hw_ns.saturating_sub(start_time.0);
                return start_time.1.elapsed() + Duration::from_nanos(elapsed_hw);
                // 粗略估算
            }
            // 兜底：直接返回当前系统时间 (不推荐，但作为 fallback)
            // 注意：这里需要计算相对于 System Boot 的 duration
            return self.instant_to_duration(arrival_time);
        }

        // 3. 计算线性回归 (y = kx + b)
        // x = hw_timestamp (relative to first point in window)
        // y = system_time (relative to first point in window)
        // 我们需要预测当前 x 对应的 y
        self.recalculate_regression();

        // 4. 应用矫正
        let (base_hw, base_sys) = self.history.front().unwrap();
        let dx = (hw_ns as f64) - (*base_hw as f64);
        let predicted_dy_ns = self.estimated_slope * dx + self.estimated_offset;

        let base_sys_dur = self.instant_to_duration(*base_sys);
        base_sys_dur + Duration::from_nanos(predicted_dy_ns as u64)
    }

    /// 简单的最小二乘法实现
    fn recalculate_regression(&mut self) {
        let n = self.history.len() as f64;
        let (base_hw, base_sys) = self.history.front().unwrap();
        let base_sys_scalar = self.instant_to_scalar(*base_sys);

        let mut sum_x = 0.0;
        let mut sum_y = 0.0;
        let mut sum_xy = 0.0;
        let mut sum_xx = 0.0;

        for (hw, sys) in &self.history {
            let x = (*hw as f64) - (*base_hw as f64);
            let y = self.instant_to_scalar(*sys) - base_sys_scalar;

            sum_x += x;
            sum_y += y;
            sum_xy += x * y;
            sum_xx += x * x;
        }

        let denominator = n * sum_xx - sum_x * sum_x;
        if denominator.abs() < 1e-6 {
            // 避免除零 (比如时间戳完全没变)
            self.estimated_slope = 1.0;
            self.estimated_offset = 0.0;
        } else {
            self.estimated_slope = (n * sum_xy - sum_x * sum_y) / denominator;
            self.estimated_offset = (sum_y * sum_xx - sum_x * sum_xy) / denominator;
        }
    }

    // 辅助：将 Instant 转为 f64 (秒), 仅用于计算差值
    fn instant_to_scalar(&self, t: Instant) -> f64 {
        // 这里实际上只需要相对值，不需要绝对 epoch
        // 我们可以用 t.elapsed() 的反向值，或者既然在一个进程内，
        // 我们统一参考 lazy_static 的启动时间点会更准。
        // 为简化代码，这里假设 t 是单调的，转换成纳秒 float。
        // 在实际生产代码中，应使用 Duration since Boot。
        // t.elapsed().as_secs_f64() // 这是一个负相关的量，有点 tricky。
        // 修正：应该保存一个 Process Start Time。
        // 这里暂略，仅展示逻辑框架。
        // 0.0

        // 获取启动时间锚点（如果还没初始化，这里会初始化为当前时间）
        let start_time = PROCESS_START.get_or_init(Instant::now);

        if t >= *start_time {
            // 正常情况：t 在启动时间之后
            t.duration_since(*start_time).as_secs_f64()
        } else {
            // 边缘情况：t 在启动时间之前（极少见，但为了数学严谨性）
            // 返回负数
            -(start_time.duration_since(t).as_secs_f64())
        }
    }

    fn instant_to_duration(&self, t: Instant) -> Duration {
        // 将 Instant 转换为 "System Up Time" (CLOCK_MONOTONIC)
        // 这是一个平台相关的操作。
        // 在 Linux 上 Instant 已经是 monotonic。
        // 我们简单返回 elapsed。实际上应该对齐到 UNIX EPOCH 或 Boot Time。
        // 这是一个 Placeholder。
        // Duration::from_nanos(0)

        // 第一次调用时会执行 Instant::now()，后续调用直接返回该值
        let anchor = PROCESS_START_TIME.get_or_init(Instant::now);

        // 使用 saturating_duration_since 防止在极端边缘情况下 (t 早于 anchor) panic
        t.saturating_duration_since(*anchor)
    }
}
