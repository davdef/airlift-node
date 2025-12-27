# 1. AlsaProducer utc_ns_now() public machen
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
    
    pub fn utc_ns_now() -> u64 {
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

# 2. File Producer fixen
cat > src/producers/file.rs << 'EOF'
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
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
    
    fn utc_ns_now() -> u64 {
        let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        d.as_secs() * 1_000_000_000 + d.subsec_nanos() as u64
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
                            utc_ns: FileProducer::utc_ns_now(),
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

# 3. Alsa Modul public export
cat > src/producers/alsa/mod.rs << 'EOF'
mod scanner;
pub mod producer;

pub use scanner::AlsaDeviceScanner;
pub use producer::AlsaProducer;
EOF

# 4. Utils für Timestamps (optional, aber sauberer)
cat > src/core/timestamp.rs << 'EOF'
use std::time::{SystemTime, UNIX_EPOCH};

pub fn utc_ns_now() -> u64 {
    let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    d.as_secs() * 1_000_000_000 + d.subsec_nanos() as u64
}

pub fn format_utc_ns(utc_ns: u64) -> String {
    let seconds = utc_ns / 1_000_000_000;
    let nanos = utc_ns % 1_000_000_000;
    format!("{}.{:09}", seconds, nanos)
}

pub fn ns_since_midnight(utc_ns: u64) -> u64 {
    let seconds_since_epoch = utc_ns / 1_000_000_000;
    let seconds_in_day = 24 * 60 * 60;
    let seconds_since_midnight = seconds_since_epoch % seconds_in_day;
    seconds_since_midnight * 1_000_000_000 + (utc_ns % 1_000_000_000)
}
EOF

# 5. Core Modul timestamp import
cat > src/core/mod.rs << 'EOF'
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use anyhow::Result;
use log::{info, error, warn};

pub mod device_scanner;
pub mod ringbuffer;
pub mod timestamp;

pub use ringbuffer::*;
pub use timestamp::*;

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

# 6. Producers auf timestamp Modul umstellen
# ALSA Producer
sed -i 's/Self::utc_ns_now()/crate::core::utc_ns_now()/g' src/producers/alsa/producer.rs

# File Producer
sed -i 's/FileProducer::utc_ns_now()/crate::core::utc_ns_now()/g' src/producers/file.rs

# 7. Build
echo "Building with proper timestamp handling..."
cargo build && echo "Erfolg! Teste mit: cargo run"
