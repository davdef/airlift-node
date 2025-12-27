use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioDeviceInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub device_type: DeviceType,
    pub supported_formats: Vec<AudioFormat>,
    pub default_format: Option<AudioFormat>,
    pub max_channels: u8,
    pub supported_rates: Vec<u32>,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DeviceType {
    Input,
    Output,
    Duplex,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioFormat {
    pub sample_rate: u32,
    pub channels: u8,
    pub sample_type: SampleType,
    pub bit_depth: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SampleType {
    SignedInteger,
    Float,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceTestResult {
    pub device_id: String,
    pub test_passed: bool,
    pub detected_format: Option<AudioFormat>,
    pub channel_peaks: Vec<f32>,
    pub channel_rms: Vec<f32>,
    pub noise_level: f32,
    pub clipping_detected: bool,
    pub estimated_latency_ms: Option<f32>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

pub trait DeviceScanner: Send + Sync {
    fn scan_devices(&self) -> Result<Vec<AudioDeviceInfo>>;
    fn test_device(&self, device_id: &str, test_duration_ms: u64) -> Result<DeviceTestResult>;
}

pub fn format_to_string(format: &AudioFormat) -> String {
    format!(
        "{}-bit {} @ {}Hz, {} channel{}",
        format.bit_depth,
        match format.sample_type {
            SampleType::SignedInteger => "SInt",
            SampleType::Float => "Float",
        },
        format.sample_rate,
        format.channels,
        if format.channels > 1 { "s" } else { "" }
    )
}
