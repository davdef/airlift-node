# 1. RingBuffer implementieren (optimiert)
cat > src/core/ringbuffer.rs << 'EOF'
use std::sync::{Arc, Mutex};
use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub struct PcmFrame {
    pub utc_ns: u64,
    pub samples: Vec<i16>,
    pub sample_rate: u32,
    pub channels: u8,
}

pub struct AudioRingBuffer {
    buffer: Arc<Mutex<VecDeque<PcmFrame>>>,
    capacity: usize,
    dropped_frames: Arc<Mutex<u64>>,
}

impl AudioRingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: Arc::new(Mutex::new(VecDeque::with_capacity(capacity))),
            capacity,
            dropped_frames: Arc::new(Mutex::new(0)),
        }
    }
    
    pub fn push(&self, frame: PcmFrame) -> u64 {
        let mut buffer = self.buffer.lock().unwrap();
        
        if buffer.len() >= self.capacity {
            buffer.pop_front();
            let mut dropped = self.dropped_frames.lock().unwrap();
            *dropped += 1;
        }
        
        buffer.push_back(frame);
        buffer.len() as u64
    }
    
    pub fn pop(&self) -> Option<PcmFrame> {
        let mut buffer = self.buffer.lock().unwrap();
        buffer.pop_front()
    }
    
    pub fn clear(&self) {
        let mut buffer = self.buffer.lock().unwrap();
        buffer.clear();
    }
    
    pub fn len(&self) -> usize {
        let buffer = self.buffer.lock().unwrap();
        buffer.len()
    }
    
    pub fn is_empty(&self) -> bool {
        let buffer = self.buffer.lock().unwrap();
        buffer.is_empty()
    }
    
    pub fn stats(&self) -> RingBufferStats {
        let buffer = self.buffer.lock().unwrap();
        let dropped = *self.dropped_frames.lock().unwrap();
        
        RingBufferStats {
            capacity: self.capacity,
            current_frames: buffer.len(),
            dropped_frames: dropped,
            latest_timestamp: buffer.back().map(|f| f.utc_ns),
            oldest_timestamp: buffer.front().map(|f| f.utc_ns),
        }
    }
    
    pub fn iter(&self) -> RingBufferIter {
        RingBufferIter {
            buffer: self.buffer.clone(),
            index: 0,
        }
    }
}

pub struct RingBufferIter {
    buffer: Arc<Mutex<VecDeque<PcmFrame>>>,
    index: usize,
}

impl Iterator for RingBufferIter {
    type Item = PcmFrame;
    
    fn next(&mut self) -> Option<Self::Item> {
        let buffer = self.buffer.lock().unwrap();
        if self.index < buffer.len() {
            // Clone the frame at index
            let frame = buffer.get(self.index).cloned();
            self.index += 1;
            frame
        } else {
            None
        }
    }
}

#[derive(Debug, Clone)]
pub struct RingBufferStats {
    pub capacity: usize,
    pub current_frames: usize,
    pub dropped_frames: u64,
    pub latest_timestamp: Option<u64>,
    pub oldest_timestamp: Option<u64>,
}
EOF

# 2. Node mit RingBuffer erweitern
cat > src/core/mod.rs << 'EOF'
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use anyhow::Result;
use log::{info, error, warn};

pub mod device_scanner;
pub mod ringbuffer;

pub use ringbuffer::*;

pub trait Producer: Send + Sync {
    fn name(&self) -> &str;
    fn start(&mut self) -> Result<()>;
    fn stop(&mut self) -> Result<()>;
    fn status(&self) -> ProducerStatus;
    fn attach_ring_buffer(&mut self, buffer: Arc<AudioRingBuffer>);
}

#[derive(Debug, Clone)]
pub struct ProducerStatus {
    pub running: bool,
    pub connected: bool,
    pub samples_processed: u64,
    pub errors: u64,
    pub buffer_stats: Option<RingBufferStats>,
}

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
    ring_buffers: Vec<Arc<AudioRingBuffer>>,
}

impl AirliftNode {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            start_time: Instant::now(),
            producers: Vec::new(),
            ring_buffers: Vec::new(),
        }
    }
    
    pub fn add_producer(&mut self, producer: Box<dyn Producer>) {
        // RingBuffer für diesen Producer erstellen
        let buffer = Arc::new(AudioRingBuffer::new(100)); // 100 Frames Kapazität
        
        // Producer mit Buffer verbinden
        let mut producer = producer;
        producer.attach_ring_buffer(buffer.clone());
        
        self.ring_buffers.push(buffer);
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
    
    pub fn producers(&self) -> &[Box<dyn Producer>] {
        &self.producers
    }
    
    pub fn ring_buffer(&self, index: usize) -> Option<&Arc<AudioRingBuffer>> {
        self.ring_buffers.get(index)
    }
    
    pub fn ring_buffers(&self) -> &[Arc<AudioRingBuffer>] {
        &self.ring_buffers
    }
}
EOF

# 3. ALSA Producer für RingBuffer anpassen
cat > src/producers/alsa/producer.rs << 'EOF'
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use anyhow::{Context, Result};

pub struct AlsaProducer {
    name: String,
    running: Arc<AtomicBool>,
    samples_processed: Arc<AtomicU64>,
    config: crate::config::ProducerConfig,
    thread_handle: Option<std::thread::JoinHandle<()>>,
    ring_buffer: Option<Arc<crate::core::AudioRingBuffer>>,
    sample_rate: u32,
    channels: u8,
}

impl AlsaProducer {
    pub fn new(name: &str, config: &crate::config::ProducerConfig) -> Result<Self> {
        let sample_rate = config.sample_rate.unwrap_or(44100);
        let channels = config.channels.unwrap_or(2);
        
        Ok(Self {
            name: name.to_string(),
            running: Arc::new(AtomicBool::new(false)),
            samples_processed: Arc::new(AtomicU64::new(0)),
            config: config.clone(),
            thread_handle: None,
            ring_buffer: None,
            sample_rate,
            channels,
        })
    }
    
    fn utc_ns_now() -> u64 {
        let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        d.as_secs() * 1_000_000_000 + d.subsec_nanos() as u64
    }
}

impl crate::core::Producer for AlsaProducer {
    fn name(&self) -> &str {
        &self.name
    }
    
    fn start(&mut self) -> Result<()> {
        if self.running.load(Ordering::Relaxed) {
            return Ok(());
        }
        
        log::info!("ALSA producer '{}' starting...", self.name);
        
        let device = self.config.device
            .clone()
            .unwrap_or_else(|| "default".to_string());
        
        log::info!("ALSA config: device={}, rate={}, channels={}", 
            device, self.sample_rate, self.channels);
        
        self.running.store(true, Ordering::SeqCst);
        
        // Thread für Audio-Aufnahme
        let running = self.running.clone();
        let samples_processed = self.samples_processed.clone();
        let name = self.name.clone();
        let ring_buffer = self.ring_buffer.clone();
        let sample_rate = self.sample_rate;
        let channels = self.channels;
        
        let handle = std::thread::spawn(move || {
            if let Err(e) = Self::run_alsa_capture(
                &device, sample_rate, channels as u32, 
                running.clone(), samples_processed.clone(),
                ring_buffer,
            ) {
                log::error!("ALSA producer '{}' error: {}", name, e);
            }
            log::info!("ALSA producer '{}' thread stopped", name);
        });
        
        self.thread_handle = Some(handle);
        Ok(())
    }
    
    fn stop(&mut self) -> Result<()> {
        log::info!("ALSA producer '{}' stopping...", self.name);
        self.running.store(false, Ordering::SeqCst);
        
        if let Some(handle) = self.thread_handle.take() {
            if let Err(e) = handle.join() {
                log::error!("Failed to join ALSA thread: {:?}", e);
            }
        }
        
        Ok(())
    }
    
    fn status(&self) -> crate::core::ProducerStatus {
        crate::core::ProducerStatus {
            running: self.running.load(Ordering::Relaxed),
            connected: true,
            samples_processed: self.samples_processed.load(Ordering::Relaxed),
            errors: 0,
            buffer_stats: self.ring_buffer.as_ref().map(|b| b.stats()),
        }
    }
    
    fn attach_ring_buffer(&mut self, buffer: Arc<crate::core::AudioRingBuffer>) {
        self.ring_buffer = Some(buffer);
    }
}

impl AlsaProducer {
    fn run_alsa_capture(
        device: &str,
        sample_rate: u32,
        channels: u32,
        running: Arc<AtomicBool>,
        samples_processed: Arc<AtomicU64>,
        ring_buffer: Option<Arc<crate::core::AudioRingBuffer>>,
    ) -> Result<()> {
        use alsa::{pcm::{Access, Format, HwParams, PCM}, Direction, ValueOr};
        
        let pcm = PCM::new(device, Direction::Capture, false)
            .with_context(|| format!("Failed to open ALSA device: {}", device))?;
        
        // Hardware-Parameter setzen
        let hwp = HwParams::any(&pcm)?;
        hwp.set_access(Access::RWInterleaved)?;
        
        // Format versuchen
        let format_result = hwp.set_format(Format::s16())
            .or_else(|_| hwp.set_format(Format::s32()))
            .or_else(|_| hwp.set_format(Format::float()));
        
        if let Err(e) = format_result {
            log::error!("No supported format found: {}", e);
            anyhow::bail!("Unsupported format for device: {}", device);
        }
        
        hwp.set_channels(channels)?;
        hwp.set_rate(sample_rate, ValueOr::Nearest)?;
        
        let period_frames = hwp.set_period_size_near(480, ValueOr::Nearest)?;
        let _buffer_size = hwp.set_buffer_size_near(period_frames * 4)?;
        
        pcm.hw_params(&hwp)?;
        pcm.prepare()?;
        
        log::info!("ALSA capture started: {}Hz, {}ch, period={} frames", 
            sample_rate, channels, period_frames);
        
        if let Ok(io) = pcm.io_i16() {
            Self::capture_i16(io, period_frames as usize, channels as usize, 
                sample_rate, running, samples_processed, ring_buffer)?;
        } else {
            log::warn!("i16 capture failed, using demo mode");
            Self::capture_demo(sample_rate, channels, running, samples_processed, ring_buffer)?;
        }
        
        log::info!("ALSA capture stopped");
        Ok(())
    }
    
    fn capture_i16(
        io: alsa::pcm::IO<i16>,
        period_frames: usize,
        channels: usize,
        sample_rate: u32,
        running: Arc<AtomicBool>,
        samples_processed: Arc<AtomicU64>,
        ring_buffer: Option<Arc<crate::core::AudioRingBuffer>>,
    ) -> Result<()> {
        let target_frames = sample_rate as usize / 10; // 100ms
        let target_samples = target_frames * channels;
        
        let period_samples = period_frames * channels;
        let mut buffer = vec![0i16; period_samples];
        let mut fifo: Vec<i16> = Vec::with_capacity(target_samples * 2);
        
        while running.load(Ordering::Relaxed) {
            match io.readi(&mut buffer) {
                Ok(frames) if frames > 0 => {
                    let samples_read = frames as usize * channels;
                    let slice = &buffer[..samples_read];
                    
                    fifo.extend_from_slice(slice);
                    samples_processed.fetch_add(samples_read as u64, Ordering::Relaxed);
                    
                    // 100ms-Chunks verarbeiten
                    while fifo.len() >= target_samples {
                        let chunk_samples: Vec<i16> = fifo.drain(..target_samples).collect();
                        
                        // In RingBuffer speichern, falls vorhanden
                        if let Some(rb) = &ring_buffer {
                            let frame = crate::core::PcmFrame {
                                utc_ns: Self::utc_ns_now(),
                                samples: chunk_samples.clone(),
                                sample_rate,
                                channels: channels as u8,
                            };
                            let buffer_len = rb.push(frame);
                            
                            static mut LAST_LOG: u64 = 0;
                            unsafe {
                                let now = Self::utc_ns_now();
                                if now - LAST_LOG >= 5_000_000_000 {
                                    log::debug!("Pushed frame to buffer. Buffer size: {}", buffer_len);
                                    LAST_LOG = now;
                                }
                            }
                        }
                    }
                }
                Ok(_) => {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(e) => {
                    log::warn!("ALSA read error: {}", e);
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }
        Ok(())
    }
    
    fn capture_demo(
        sample_rate: u32,
        channels: u32,
        running: Arc<AtomicBool>,
        samples_processed: Arc<AtomicU64>,
        ring_buffer: Option<Arc<crate::core::AudioRingBuffer>>,
    ) -> Result<()> {
        log::warn!("Using demo mode - simulating audio");
        
        let target_frames = sample_rate as usize / 10; // 100ms
        let target_samples = target_frames * channels as usize;
        
        let mut tick = 0;
        while running.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(100));
            tick += 1;
            
            if tick % 10 == 0 { // Alle Sekunde
                let chunk_samples = vec![0i16; target_samples];
                samples_processed.fetch_add(target_samples as u64, Ordering::Relaxed);
                
                // In RingBuffer speichern
                if let Some(rb) = &ring_buffer {
                    let frame = crate::core::PcmFrame {
                        utc_ns: Self::utc_ns_now(),
                        samples: chunk_samples,
                        sample_rate,
                        channels: channels as u8,
                    };
                    rb.push(frame);
                }
                
                log::debug!("Demo: simulated {} samples", target_samples);
            }
        }
        
        Ok(())
    }
}
EOF

# 4. File Producer ebenfalls anpassen
cat > src/producers/file.rs << 'EOF'
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use anyhow::Result;

pub struct FileProducer {
    name: String,
    running: Arc<AtomicBool>,
    samples_processed: Arc<AtomicU64>,
    config: crate::config::ProducerConfig,
    thread_handle: Option<std::thread::JoinHandle<()>>,
    ring_buffer: Option<Arc<crate::core::AudioRingBuffer>>,
}

impl FileProducer {
    pub fn new(name: &str, config: &crate::config::ProducerConfig) -> Self {
        Self {
            name: name.to_string(),
            running: Arc::new(AtomicBool::new(false)),
            samples_processed: Arc::new(AtomicU64::new(0)),
            config: config.clone(),
            thread_handle: None,
            ring_buffer: None,
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
        let ring_buffer = self.ring_buffer.clone();
        
        std::thread::spawn(move || {
            log::info!("FileProducer '{}': Playing {}", name, path);
            
            // Simuliere Audio-Daten
            let sample_rate = 48000;
            let channels = 2;
            let target_frames = sample_rate as usize / 10; // 100ms
            let target_samples = target_frames * channels;
            
            let mut tick = 0;
            while running.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(100));
                tick += 1;
                
                samples_processed.fetch_add(100, Ordering::Relaxed);
                
                // Alle 10 Ticks (1 Sekunde) einen Frame erzeugen
                if tick % 10 == 0 {
                    let chunk_samples = vec![0i16; target_samples];
                    
                    // In RingBuffer speichern
                    if let Some(rb) = &ring_buffer {
                        let frame = crate::core::PcmFrame {
                            utc_ns: crate::producers::alsa::producer::AlsaProducer::utc_ns_now(),
                            samples: chunk_samples,
                            sample_rate,
                            channels: channels as u8,
                        };
                        rb.push(frame);
                    }
                    
                    log::debug!("FileProducer '{}': Generated frame", name);
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
            buffer_stats: self.ring_buffer.as_ref().map(|b| b.stats()),
        }
    }
    
    fn attach_ring_buffer(&mut self, buffer: Arc<crate::core::AudioRingBuffer>) {
        self.ring_buffer = Some(buffer);
    }
}
EOF

# 5. Main erweitern für RingBuffer Stats
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
    
    let args: Vec<String> = std::env::args().collect();
    
    if args.len() > 1 && args[1] == "--discover" {
        return run_discovery();
    }
    
    run_normal_mode()
}

fn run_discovery() -> anyhow::Result<()> {
    log::info!("Starting ALSA device discovery...");
    
    use crate::core::device_scanner::DeviceScanner;
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
        std::thread::sleep(Duration::from_millis(500));
        
        tick += 1;
        if tick % 10 == 0 {
            let status = node.status();
            log::info!("=== Node Status ===");
            log::info!("Running: {}, Uptime: {}s, Producers: {}", 
                status.running, status.uptime_seconds, status.producers);
            
            // Producer Status anzeigen
            for (i, producer) in node.producers().iter().enumerate() {
                let p_status = producer.status();
                log::info!("  [{:2}] {}:", i, producer.name());
                log::info!("       running={}, connected={}, samples={}, errors={}", 
                    p_status.running,
                    p_status.connected,
                    p_status.samples_processed,
                    p_status.errors
                );
                
                // RingBuffer Stats
                if let Some(stats) = &p_status.buffer_stats {
                    log::info!("       buffer: {}/{} frames, dropped={}", 
                        stats.current_frames, stats.capacity, stats.dropped_frames);
                }
                
                // RingBuffer direkt abfragen
                if let Some(rb) = node.ring_buffer(i) {
                    let stats = rb.stats();
                    log::info!("       ringbuf: {} frames stored", stats.current_frames);
                }
            }
        }
    }
    
    node.stop()?;
    log::info!("Node stopped");
    
    Ok(())
}
EOF

# 6. Build und Test
echo "Building with RingBuffer support..."
cargo build && echo "Teste mit: cargo run"
