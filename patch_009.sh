# 1. Config korrigieren
cat > config.toml << 'EOF'
node_name = "studio-node"

[producers.mic1]
type = "alsa"
enabled = true
device = "dsnoop:CARD=SoundCard,DEV=0"
channels = 2
sample_rate = 48000

[producers.background_music]
type = "file"
enabled = true
path = "background.wav"
loop_audio = true
EOF

# 2. Config-Parser anpassen (field name)
cat > src/config/mod.rs << 'EOF'
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;

#[derive(Debug, Clone, Deserialize)]
pub struct ProducerConfig {
    #[serde(rename = "type")]
    pub producer_type: String,
    pub enabled: bool,
    pub device: Option<String>,
    pub path: Option<String>,
    pub channels: Option<u8>,
    pub sample_rate: Option<u32>,
    pub loop_audio: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub node_name: String,
    pub producers: HashMap<String, ProducerConfig>,
}

impl Config {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let content = fs::read_to_string(path)?;
        Ok(toml::from_str(&content)?)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            node_name: "airlift-node".to_string(),
            producers: HashMap::new(),
        }
    }
}
EOF

# 3. Main Producer Status nutzen (Warnings beseitigen)
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
        std::thread::sleep(Duration::from_millis(500)); // Langsamer
        
        tick += 1;
        if tick % 10 == 0 { // Alle 5 Sekunden
            let status = node.status();
            log::info!("=== Node Status ===");
            log::info!("Running: {}, Uptime: {}s, Producers: {}", 
                status.running, status.uptime_seconds, status.producers);
            
            // Producer Status anzeigen
            for producer in &node.producers {
                let p_status = producer.status();
                log::info!("  - {}: running={}, connected={}, samples={}, errors={}", 
                    producer.name(),
                    p_status.running,
                    p_status.connected,
                    p_status.samples_processed,
                    p_status.errors
                );
            }
        }
    }
    
    node.stop()?;
    log::info!("Node stopped");
    
    Ok(())
}
EOF

# 4. Core-Modul unused imports entfernen
cat > src/core/mod.rs << 'EOF'
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use anyhow::Result;
use log::{info, error, warn};

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

# 5. RingBuffer vorerst entfernen (später verwenden)
rm src/core/ringbuffer.rs

# 6. Producers Modul cleanen
cat > src/producers/mod.rs << 'EOF'
pub mod file;
pub mod alsa;
EOF

# 7. Test!
echo "Testing ALSA producer..."
cargo run
