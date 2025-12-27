// src/core/device_discovery.rs

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioDeviceInfo {
    pub id: String,
    pub name: String,
    pub device_type: DeviceType,
    pub formats: Vec<AudioFormat>,
    pub channels: Vec<u8>,
    pub sample_rates: Vec<u32>,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeviceType {
    Input,
    Output,
    Duplex,
}

pub trait DeviceScanner: Send + Sync {
    /// Scannt alle verfügbaren Audio-Geräte
    fn scan_devices(&self) -> Result<Vec<AudioDeviceInfo>>;
    
    /// Testet ein spezifisches Gerät (Signal-Prüfung)
    fn test_device(&self, device_id: &str, duration_ms: u64) -> Result<DeviceTestResult>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceTestResult {
    pub device_id: String,
    pub success: bool,
    pub detected_format: Option<AudioFormat>,
    pub peak_levels: Vec<f32>,  // pro Kanal
    pub noise_floor: f32,
    pub latency_ms: Option<f32>,
    pub errors: Vec<String>,
}
