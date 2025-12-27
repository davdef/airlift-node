#!/bin/bash

# 1. ALSA Scanner fixen
cat > src/producers/alsa/scanner.rs << 'EOF'
use anyhow::{Context, Result};
use std::ffi::CStr;
use crate::core::device_scanner::*;

pub struct AlsaDeviceScanner;

impl DeviceScanner for AlsaDeviceScanner {
    fn scan_devices(&self) -> Result<Vec<AudioDeviceInfo>> {
        use alsa::{device_name::HintIter, Direction};
        
        let mut devices = Vec::new();
        
        // Scan PCM devices using ALSA hints
        let hints = HintIter::new(None, CStr::from_bytes_with_nul(b"pcm\0").unwrap())
            .context("Failed to get ALSA device hints")?;
        
        for hint in hints {
            let Some(name) = hint.name else { continue };
            let desc = hint.desc.unwrap_or_default();
            let Some(direction) = hint.direction else { continue };
            
            // Determine device type
            let device_type = match direction {
                Direction::Capture => DeviceType::Input,
                Direction::Playback => DeviceType::Output,
            };
            
            // Try to get hardware parameters
            let supported_formats = match self.get_supported_formats(&name) {
                Ok(fmts) => fmts,
                Err(e) => {
                    log::debug!("Failed to get formats for {}: {}", name, e);
                    Vec::new()
                }
            };
            
            let max_channels = self.get_max_channels(&name).unwrap_or(2);
            
            devices.push(AudioDeviceInfo {
                id: name.clone(),
                name: desc.clone(),
                description: format!("ALSA: {}", desc),
                device_type,
                supported_formats,
                default_format: None,
                max_channels,
                supported_rates: vec![44100, 48000, 96000, 192000],
                is_default: name == "default" || name.contains("default"),
            });
        }
        
        Ok(devices)
    }
    
    fn test_device(&self, device_id: &str, _test_duration_ms: u64) -> Result<DeviceTestResult> {
        use alsa::{pcm::PCM, Direction};
        
        log::info!("Testing ALSA device: {}", device_id);
        
        // Try to open the device
        let pcm = PCM::new(device_id, Direction::Capture, false)
            .with_context(|| format!("Failed to open device: {}", device_id))?;
        
        let hw_params = pcm.hw_params_current()?;
        
        // Collect format information
        let channels = hw_params.get_channels_max() as u8;
        let min_rate = hw_params.get_rate_min()?;
        let max_rate = hw_params.get_rate_max()?;
        
        let mut warnings = Vec::new();
        
        // Basic test: can we configure it?
        let mut test_passed = false;
        let mut detected_format = None;
        
        // Try common formats
        for rate in &[44100, 48000, 96000] {
            if *rate >= min_rate && *rate <= max_rate {
                test_passed = true;
                detected_format = Some(AudioFormat {
                    sample_rate: *rate,
                    channels: channels.min(2),
                    sample_type: SampleType::SignedInteger,
                    bit_depth: 16,
                });
                break;
            }
        }
        
        if !test_passed {
            warnings.push(format!("No supported sample rate found. Min: {}, Max: {}", 
                min_rate, max_rate));
        }
        
        Ok(DeviceTestResult {
            device_id: device_id.to_string(),
            test_passed,
            detected_format,
            channel_peaks: vec![0.0; channels as usize],
            channel_rms: vec![0.0; channels as usize],
            noise_level: 0.0,
            clipping_detected: false,
            estimated_latency_ms: Some(10.0),
            warnings,
            errors: Vec::new(),
        })
    }
}

impl AlsaDeviceScanner {
    fn get_supported_formats(&self, device_id: &str) -> Result<Vec<AudioFormat>> {
        use alsa::{pcm::PCM, Direction};
        
        let mut formats = Vec::new();
        
        // Try to open device to query formats
        let pcm = PCM::new(device_id, Direction::Capture, false)?;
        let hw_params = pcm.hw_params_current()?;
        
        // Common sample rates
        let sample_rates = [44100, 48000, 96000, 192000];
        
        for &rate in &sample_rates {
            if hw_params.test_rate(rate).is_ok() {
                formats.push(AudioFormat {
                    sample_rate: rate,
                    channels: 2,
                    sample_type: SampleType::SignedInteger,
                    bit_depth: 16,
                });
            }
        }
        
        Ok(formats)
    }
    
    fn get_max_channels(&self, device_id: &str) -> Result<u8> {
        use alsa::{pcm::PCM, Direction};
        
        let pcm = PCM::new(device_id, Direction::Capture, false)?;
        let hw_params = pcm.hw_params_current()?;
        Ok(hw_params.get_channels_max() as u8)
    }
}
EOF

# 2. Main korrigieren (Trait import + Typannotation)
cat > src/main.rs << 'EOF'
mod core;
mod config;
mod producers;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use crate::core::device_scanner::DeviceScanner;

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();
    
    log::info!("=== Airlift Node v0.2.0 ===");
    
    let args: Vec<String> = std::env::args().collect();
    
    if args.len() > 1 && args[1] == "--discover" {
        return run_discovery();
    }
    
    run_normal_mode()
}

fn run_discovery() -> anyhow::Result<()> {
    log::info!("Starting ALSA device discovery...");
    
    let scanner = producers::alsa::AlsaDeviceScanner;
    
    match scanner.scan_devices() {
        Ok(devices) => {
            log::info!("Found {} audio devices", devices.len());
            
            let json = serde_json::to_string_pretty(&devices)?;
            println!("{}", json);
            
            for device in &devices {
                log::info!("Device: {} ({}) - {}", 
                    device.name, 
                    device.id,
                    match device.device_type {
                        crate::core::device_scanner::DeviceType::Input => "Input",
                        crate::core::device_scanner::DeviceType::Output => "Output",
                        crate::core::device_scanner::DeviceType::Duplex => "Duplex",
                    }
                );
            }
        }
        Err(e) => {
            log::error!("Failed to scan devices: {}", e);
            anyhow::bail!("Discovery failed: {}", e);
        }
    }
    
    Ok(())
}

fn run_normal_mode() -> anyhow::Result<()> {
    let config = config::Config::load("config.toml")
        .unwrap_or_else(|e| {
            log::warn!("Config error: {}, using defaults", e);
            config::Config::default()
        });
    
    log::info!("Node: {}", config.node_name);
    
    let mut node = core::AirliftNode::new();
    
    for (name, producer_cfg) in &config.producers {
        if !producer_cfg.enabled {
            continue;
        }
        
        match producer_cfg.producer_type.as_str() {
            "file" => {
                let producer = producers::file::FileProducer::new(name, producer_cfg);
                node.add_producer(Box::new(producer));
                log::info!("Added file producer: {}", name);
            }
            "alsa" => {
                match producers::alsa::AlsaProducer::new(name, producer_cfg) {
                    Ok(producer) => {
                        node.add_producer(Box::new(producer));
                        log::info!("Added ALSA producer: {}", name);
                    }
                    Err(e) => {
                        log::error!("Failed to create ALSA producer {}: {}", name, e);
                    }
                }
            }
            _ => log::error!("Unknown producer type: {}", producer_cfg.producer_type),
        }
    }
    
    if node.status().producers == 0 {
        log::info!("No producers configured, adding demo");
        let demo_cfg = config::ProducerConfig {
            producer_type: "file".to_string(),
            enabled: true,
            device: None,
            path: Some("demo.wav".to_string()),
            channels: Some(2),
            sample_rate: Some(48000),
            loop_audio: Some(true),
        };
        let demo_producer = producers::file::FileProducer::new("demo", &demo_cfg);
        node.add_producer(Box::new(demo_producer));
    }
    
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
EOF

# 3. ALSA Modul den Trait importieren lassen
cat > src/producers/alsa/mod.rs << 'EOF'
mod scanner;
pub use scanner::AlsaDeviceScanner;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use anyhow::Result;
use crate::core::device_scanner::DeviceScanner;

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
            connected: false,
            samples_processed: self.samples_processed.load(Ordering::Relaxed),
            errors: 0,
        }
    }
}
EOF

# 4. Build testen
cargo run -- --discover
