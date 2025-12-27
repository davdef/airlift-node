#!/bin/bash

# 1. Scanner Modul erstellen
mkdir -p src/producers/alsa
touch src/producers/alsa/scanner.rs

# 2. Core-Module korrigieren
cat > src/core/mod.rs << 'EOF'
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use anyhow::Result;
use log::{info, error, warn};

// ============================================================================
// RINGBUFFER (von deinem Code übernommen)
// ============================================================================

mod ringbuffer;
pub use ringbuffer::*;

// ============================================================================
// DEVICE SCANNER
// ============================================================================

pub mod device_scanner;
pub use device_scanner::*;

// ============================================================================
// PRODUCER TRAIT
// ============================================================================

pub trait Producer: Send + Sync {
    fn name(&self) -> &str;
    fn start(&mut self) -> Result<()>;
    fn stop(&mut self) -> Result<()>;
    fn status(&self) -> ProducerStatus;
}

#[derive(Debug, Clone)]
pub struct ProducerStatus {
    pub running: bool,
    pub connected: bool,
    pub samples_processed: u64,
    pub errors: u64,
}

// ============================================================================
// AIRLIFT NODE
// ============================================================================

#[derive(Debug)]
pub struct NodeStatus {
    pub running: bool,
    pub uptime_seconds: u64,
    pub producers: usize,
}

pub struct AirliftNode {
    running: Arc<AtomicBool>,
    start_time: Instant,
    producers: Vec<Box<dyn Producer>>,
}

impl AirliftNode {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            start_time: Instant::now(),
            producers: Vec::new(),
        }
    }
    
    pub fn add_producer(&mut self, producer: Box<dyn Producer>) {
        self.producers.push(producer);
    }
    
    pub fn start(&mut self) -> Result<()> {
        info!("Node starting...");
        self.running.store(true, Ordering::SeqCst);
        
        for (i, producer) in self.producers.iter_mut().enumerate() {
            info!("Starting producer {}: {}", i, producer.name());
            if let Err(e) = producer.start() {
                error!("Failed to start producer {}: {}", producer.name(), e);
            }
        }
        
        Ok(())
    }
    
    pub fn stop(&mut self) -> Result<()> {
        info!("Node stopping...");
        self.running.store(false, Ordering::SeqCst);
        
        for producer in &mut self.producers {
            info!("Stopping producer: {}", producer.name());
            if let Err(e) = producer.stop() {
                warn!("Error stopping producer {}: {}", producer.name(), e);
            }
        }
        
        Ok(())
    }
    
    pub fn status(&self) -> NodeStatus {
        NodeStatus {
            running: self.running.load(Ordering::Relaxed),
            uptime_seconds: self.start_time.elapsed().as_secs(),
            producers: self.producers.len(),
        }
    }
    
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
}
EOF

# 3. Device Scanner Modul erstellen
cat > src/core/device_scanner.rs << 'EOF'
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
EOF

# 4. Ringbuffer Modul (vereinfacht für jetzt)
cat > src/core/ringbuffer.rs << 'EOF'
// Vereinfachte Version für den Start
// Später deinen vollständigen Code integrieren

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

pub struct RingStats {
    pub capacity: usize,
    pub head_seq: u64,
    pub next_seq: u64,
    pub arc_replacements: u64,
}

// Für jetzt nur Platzhalter
pub struct PcmRingBuffer;

impl PcmRingBuffer {
    pub fn new(_slots: usize, _prealloc_samples: usize) -> Self {
        Self
    }
}
EOF

# 5. Config Default implementieren
cat > src/config/mod.rs << 'EOF'
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct Config {
    #[serde(default = "default_node_name")]
    pub node_name: String,
    
    #[serde(default)]
    pub producers: HashMap<String, ProducerConfig>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct ProducerConfig {
    #[serde(rename = "type")]
    pub producer_type: String,
    
    #[serde(default = "default_true")]
    pub enabled: bool,
    
    #[serde(default)]
    pub device: Option<String>,
    
    #[serde(default)]
    pub path: Option<String>,
    
    #[serde(default)]
    pub channels: Option<u8>,
    
    #[serde(default)]
    pub sample_rate: Option<u32>,
    
    #[serde(default)]
    pub loop_audio: Option<bool>,
}

fn default_node_name() -> String {
    "airlift-node".to_string()
}

fn default_true() -> bool {
    true
}

impl Config {
    pub fn load(path: &str) -> Result<Self, anyhow::Error> {
        if std::path::Path::new(path).exists() {
            let content = std::fs::read_to_string(path)?;
            Ok(toml::from_str(&content)?)
        } else {
            log::warn!("Config file '{}' not found, using defaults", path);
            Ok(Self::default())
        }
    }
}
EOF

# 6. Main korrigieren (ohne Discovery für jetzt)
cat > src/main.rs << 'EOF'
mod core;
mod config;
mod producers;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();
    
    log::info!("=== Airlift Node v0.2.0 ===");
    
    // Einfacher Modus: Discovery oder normal
    let args: Vec<String> = std::env::args().collect();
    
    if args.len() > 1 && args[1] == "--discover" {
        return run_discovery();
    }
    
    // Normaler Modus
    let config = config::Config::load("config.toml")
        .unwrap_or_else(|e| {
            log::warn!("Config error: {}, using defaults", e);
            config::Config::default()
        });
    
    log::info!("Node: {}", config.node_name);
    
    let mut node = core::AirliftNode::new();
    
    // Producer aus Config
    for (name, producer_cfg) in &config.producers {
        if !producer_cfg.enabled {
            continue;
        }
        
        match producer_cfg.producer_type.as_str() {
            "file" => {
                let producer = producers::file::FileProducer::new(name, producer_cfg)?;
                node.add_producer(Box::new(producer));
                log::info!("Added file producer: {}", name);
            }
            "alsa" => {
                log::warn!("ALSA producer not yet implemented: {}", name);
            }
            _ => log::error!("Unknown producer type: {}", producer_cfg.producer_type),
        }
    }
    
    // Graceful shutdown
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();
    
    ctrlc::set_handler(move || {
        log::info!("\nShutdown requested (Ctrl+C)");
        shutdown_clone.store(true, Ordering::SeqCst);
    })?;
    
    node.start()?;
    log::info!("Node started with {} producers. Press Ctrl+C to stop.", 
               node.status().producers);
    
    let mut tick = 0;
    while !shutdown.load(Ordering::Relaxed) && node.is_running() {
        std::thread::sleep(Duration::from_millis(100));
        
        tick += 1;
        if tick % 50 == 0 {
            let status = node.status();
            log::info!("Status: running={}, uptime={}s, producers={}", 
                status.running, status.uptime_seconds, status.producers);
        }
    }
    
    node.stop()?;
    log::info!("Node stopped");
    
    Ok(())
}

fn run_discovery() -> anyhow::Result<()> {
    log::info!("Device discovery not yet implemented");
    log::info!("Run 'cargo run' for normal mode");
    Ok(())
}
EOF

# 7. Producers Modul korrigieren
cat > src/producers/mod.rs << 'EOF'
pub mod file;
pub mod alsa;

// Factory für später
pub fn create_device_scanner(_scanner_type: &str) -> Option<Box<dyn crate::core::DeviceScanner>> {
    None // Für später
}
EOF

# 8. Alsa Producer vereinfachen
cat > src/producers/alsa.rs << 'EOF'
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use anyhow::Result;

pub struct AlsaProducer {
    name: String,
    running: Arc<AtomicBool>,
    samples_processed: Arc<AtomicU64>,
    config: crate::config::ProducerConfig,
}

impl AlsaProducer {
    pub fn new(name: &str, config: &crate::config::ProducerConfig) -> Result<Self> {
        Ok(Self {
            name: name.to_string(),
            running: Arc::new(AtomicBool::new(false)),
            samples_processed: Arc::new(AtomicU64::new(0)),
            config: config.clone(),
        })
    }
}

impl crate::core::Producer for AlsaProducer {
    fn name(&self) -> &str {
        &self.name
    }
    
    fn start(&mut self) -> Result<()> {
        log::info!("ALSA producer '{}' starting (not implemented)", self.name);
        self.running.store(true, Ordering::SeqCst);
        Ok(())
    }
    
    fn stop(&mut self) -> Result<()> {
        log::info!("ALSA producer '{}' stopping", self.name);
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }
    
    fn status(&self) -> crate::core::ProducerStatus {
        crate::core::ProducerStatus {
            running: self.running.load(Ordering::Relaxed),
            connected: false, // Nicht implementiert
            samples_processed: self.samples_processed.load(Ordering::Relaxed),
            errors: 0,
        }
    }
}
EOF

# 9. Scanner Modul Stub
cat > src/producers/alsa/scanner.rs << 'EOF'
use anyhow::Result;
use crate::core::device_scanner::*;

pub struct AlsaDeviceScanner;

impl DeviceScanner for AlsaDeviceScanner {
    fn scan_devices(&self) -> Result<Vec<AudioDeviceInfo>> {
        log::warn!("ALSA device scanner not yet implemented");
        Ok(vec![])
    }
    
    fn test_device(&self, device_id: &str, _test_duration_ms: u64) -> Result<DeviceTestResult> {
        log::warn!("ALSA device testing not yet implemented for {}", device_id);
        
        Ok(DeviceTestResult {
            device_id: device_id.to_string(),
            test_passed: false,
            detected_format: None,
            channel_peaks: vec![],
            channel_rms: vec![],
            noise_level: 0.0,
            clipping_detected: false,
            estimated_latency_ms: None,
            warnings: vec!["Not implemented".to_string()],
            errors: vec![],
        })
    }
}
EOF

# Build testen
cargo build
