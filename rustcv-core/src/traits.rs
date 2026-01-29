use crate::builder::CameraConfig;
use crate::error::Result;
use crate::frame::Frame;
use async_trait::async_trait;

// --- 补全缺失的结构体定义 ---

/// 设备基本信息
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceInfo {
    /// 对用户友好的显示名称 (e.g. "Logitech C920")
    pub name: String,

    /// 唯一硬件 ID (e.g. "/dev/video0" 或 USB 序列号)
    /// 用于 Driver::open 的参数
    pub id: String,

    /// 后端类型标识 (e.g. "V4L2", "MediaFoundation")
    pub backend: String,

    /// 硬件总线信息 (可选，e.g. "usb-0000:00:14.0-1")
    /// 用于高级拓扑识别
    pub bus_info: Option<String>,
}

/// 硬件触发源配置
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TriggerConfig {
    /// 触发模式
    pub mode: TriggerMode,

    /// 触发源
    pub source: TriggerSource,

    /// 触发极性/边缘
    pub polarity: TriggerPolarity,

    /// 触发延迟 (微秒)，硬件接收信号后延迟多久开始曝光
    pub delay_us: u32,
}

impl Default for TriggerConfig {
    fn default() -> Self {
        Self {
            mode: TriggerMode::Off,
            source: TriggerSource::Software,
            polarity: TriggerPolarity::RisingEdge,
            delay_us: 0,
        }
    }
}

/// 触发模式枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerMode {
    /// 关闭触发，使用连续采集模式 (Free Run)
    Off,
    /// 标准触发模式 (一帧一触发)
    Standard,
    /// 脉宽控制曝光 (Bulb 模式，曝光时间由信号宽度决定)
    Bulb,
}

/// 触发源枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerSource {
    /// 软件触发 (通过 API 调用触发)
    Software,
    /// 外部硬件线路 0 (GPIO / Opto-isolated Input)
    Line0,
    /// 外部硬件线路 1
    Line1,
    /// 外部硬件线路 2
    Line2,
    /// 外部硬件线路 3
    Line3,
}

/// 触发极性/边缘枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerPolarity {
    /// 上升沿触发
    RisingEdge,
    /// 下降沿触发
    FallingEdge,
    /// 高电平触发 (Level)
    HighLevel,
    /// 低电平触发 (Level)
    LowLevel,
}

// --- 核心 Trait 定义 (保持不变) ---

/// 1. 驱动入口：设备枚举与管理
pub trait Driver: Send + Sync {
    /// 扫描总线，返回设备列表（含唯一 ID 和拓扑路径）
    fn list_devices(&self) -> Result<Vec<DeviceInfo>>;

    /// 打开设备
    /// 返回分离的 Stream (数据面) 和 Controls (控制面)
    fn open(&self, id: &str, config: CameraConfig) -> Result<(Box<dyn Stream>, DeviceControls)>;
}

/// 2. 数据面：流式获取
/// 必须是 Send，以便在 Tokio 任务中运行
#[async_trait]
pub trait Stream: Send {
    /// 启动采集 (Alloc buffers, Start DMA)
    async fn start(&mut self) -> Result<()>;

    /// 停止采集 (Release bandwidth)
    async fn stop(&mut self) -> Result<()>;

    /// 获取下一帧
    /// 注意：这里返回的 Frame 生命周期绑定到 self (Stream)
    /// 实现 Ring Buffer 的借用语义
    async fn next_frame(&mut self) -> Result<Frame<'_>>;

    /// 【逃生舱口】直接注入虚拟帧 (用于仿真)
    #[cfg(feature = "simulation")]
    async fn inject_frame(&mut self, frame: Frame<'_>) -> Result<()>;
}

/// 3. 控制面聚合体
#[allow(missing_debug_implementations)]
pub struct DeviceControls {
    pub sensor: Box<dyn SensorControl>, // 传感器控制 (曝光, 增益)
    pub lens: Box<dyn LensControl>,     // 镜头控制 (变焦, 对焦) - 独立锁
    pub system: Box<dyn SystemControl>, // 系统控制 (复位, 触发)
}

/// 传感器控制 Trait
pub trait SensorControl: Send + Sync {
    fn set_exposure(&self, value_us: u32) -> Result<()>;
    fn get_exposure(&self) -> Result<u32>;
    // ... Gain, WhiteBalance 可以在此扩展
}

/// 镜头控制 Trait (允许并发操作，不阻塞 Sensor)
pub trait LensControl: Send + Sync {
    fn set_zoom(&self, zoom: u32) -> Result<()>;
    fn set_focus(&self, focus: u32) -> Result<()>;
}

/// 系统/高级控制 Trait
pub trait SystemControl: Send + Sync {
    /// 【硬核特性】USB 端口级复位
    /// 注意：这是一个 unsafe 操作，可能会导致其他 USB 设备短暂断连
    /// # Safety
    unsafe fn force_reset(&self) -> Result<()>;

    /// 设置硬件触发模式
    fn set_trigger(&self, config: TriggerConfig) -> Result<()>;

    /// 导出当前配置快照 (用于持久化)
    /// 返回值使用 serde_json::Value 以兼容不同后端的配置结构
    #[cfg(feature = "serialize")]
    fn export_state(&self) -> Result<serde_json::Value>;
}

// 【新增】为 Box<T> 实现 Stream，这样 Box<dyn Stream> 也能被当做 Stream 使用
#[async_trait]
impl<S: Stream + ?Sized + Send> Stream for Box<S> {
    async fn start(&mut self) -> Result<()> {
        (**self).start().await
    }

    async fn stop(&mut self) -> Result<()> {
        (**self).stop().await
    }

    async fn next_frame(&mut self) -> Result<Frame<'_>> {
        (**self).next_frame().await
    }

    #[cfg(feature = "simulation")]
    async fn inject_frame(&mut self, frame: Frame<'_>) -> Result<()> {
        (**self).inject_frame(frame).await
    }
}
