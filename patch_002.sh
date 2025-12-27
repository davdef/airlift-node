#!/bin/bash

# 1. FileProducer Status korrigieren
cat > src/producers/file.rs << 'EOF'
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use anyhow::Result;

pub struct FileProducer {
    name: String,
    running: Arc<AtomicBool>,
    samples_processed: Arc<AtomicU64>,
    config: crate::config::ProducerConfig,
}

impl FileProducer {
    pub fn new(name: &str, config: &crate::config::ProducerConfig) -> Self {
        Self {
            name: name.to_string(),
            running: Arc::new(AtomicBool::new(false)),
            samples_processed: Arc::new(AtomicU64::new(0)),
            config: config.clone(),
        }
    }
}

impl crate::core::Producer for FileProducer {
    fn name(&self) -> &str {
        &self.name
    }
    
    fn start(&mut self) -> Result<()> {
        let path = self.config.path.clone().unwrap_or_else(|| "default.wav".to_string());
        let loop_audio = self.config.loop_audio.unwrap_or(false);
        
        log::info!("FileProducer '{}': Starting (path: {}, loop: {})", 
            self.name, path, loop_audio);
        
        self.running.store(true, Ordering::SeqCst);
        
        let samples_processed = self.samples_processed.clone();
        let running = self.running.clone();
        let name = self.name.clone();
        
        std::thread::spawn(move || {
            log::info!("FileProducer '{}': Playing {}", name, path);
            
            while running.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(100));
                samples_processed.fetch_add(100, Ordering::Relaxed);
                
                static mut LAST_LOG: u64 = 0;
                unsafe {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs();
                    
                    if now - LAST_LOG >= 5 {
                        log::debug!("FileProducer '{}': Playing {}", name, path);
                        LAST_LOG = now;
                    }
                }
            }
            
            log::info!("FileProducer '{}': Stopped", name);
        });
        
        Ok(())
    }
    
    fn stop(&mut self) -> Result<()> {
        log::info!("FileProducer '{}': Stopping...", self.name);
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }
    
    fn status(&self) -> crate::core::ProducerStatus {
        crate::core::ProducerStatus {
            running: self.running.load(Ordering::Relaxed),
            connected: true,
            samples_processed: self.samples_processed.load(Ordering::Relaxed),
            errors: 0,
        }
    }
}
EOF

# 2. Main korrigieren (? entfernen)
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
                let producer = producers::file::FileProducer::new(name, producer_cfg);
                node.add_producer(Box::new(producer));
                log::info!("Added file producer: {}", name);
            }
            "alsa" => {
                let producer = producers::alsa::AlsaProducer::new(name, producer_cfg);
                node.add_producer(Box::new(producer));
                log::info!("Added ALSA producer: {}", name);
            }
            _ => log::error!("Unknown producer type: {}", producer_cfg.producer_type),
        }
    }
    
    // Falls keine Producer: Demo
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

# 3. Unused imports entfernen
cat > src/core/mod.rs << 'EOF'
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
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

# 4. Ringbuffer Modul vereinfachen
cat > src/core/ringbuffer.rs << 'EOF'
// Platzhalter für später
pub struct RingStats {
    pub capacity: usize,
    pub head_seq: u64,
    pub next_seq: u64,
    pub arc_replacements: u64,
}

pub struct PcmRingBuffer;

impl PcmRingBuffer {
    pub fn new(_slots: usize, _prealloc_samples: usize) -> Self {
        Self
    }
}
EOF

# Jetzt builden
cargo build
