use std::fmt;

/// 设备健康状况与遥测数据
///
/// 这些数据通常不随每一帧变化，而是作为 DeviceControls 的一部分定期查询，
/// 或者作为 FrameMetadata 的扩展。
#[derive(Clone, Default, PartialEq)]
pub struct DeviceTelemetry {
    /// 芯片核心温度 (摄氏度)
    /// 如果过高，意味着可能发生了热节流 (Thermal Throttling)
    pub temperature_c: Option<f32>,

    /// 当前 USB/网络链路的实际吞吐量 (Mbps)
    pub link_throughput_mbps: Option<u32>,

    /// 传输层丢包/误码计数
    /// 比如 USB 的 Isochronous Packet Errors 或 GigE 的 Packet Resend
    pub transmission_errors: u64,

    /// 丢帧计数器 (因缓冲区溢出或 CPU 处理慢)
    pub dropped_frames: u64,

    /// 损坏帧计数器 (因 Payload 校验失败)
    pub corrupted_frames: u64,

    /// 当前功耗估算 (毫瓦/mW)
    pub power_consumption_mw: Option<u32>,
}

impl fmt::Debug for DeviceTelemetry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceTelemetry")
            .field("temp_c", &self.temperature_c.unwrap_or(-1.0))
            .field("link_mbps", &self.link_throughput_mbps.unwrap_or(0))
            .field("errors", &self.transmission_errors)
            .field("dropped", &self.dropped_frames)
            .finish()
    }
}

/// 简单的状态指示灯
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceHealthStatus {
    Healthy,
    Warning(HealthIssue),
    Critical(HealthIssue),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthIssue {
    Overheating,
    BandwidthSaturation,
    HighPacketLoss,
    SensorError,
}

impl DeviceTelemetry {
    /// 基于遥测数据简单的健康评估
    pub fn assess_health(&self) -> DeviceHealthStatus {
        if let Some(t) = self.temperature_c {
            if t > 85.0 {
                return DeviceHealthStatus::Critical(HealthIssue::Overheating);
            } else if t > 75.0 {
                return DeviceHealthStatus::Warning(HealthIssue::Overheating);
            }
        }

        if self.transmission_errors > 100 {
            return DeviceHealthStatus::Warning(HealthIssue::HighPacketLoss);
        }

        DeviceHealthStatus::Healthy
    }
}
