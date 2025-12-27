# 1. RingBuffer wieder einführen (vereinfacht)
cat > src/core/ringbuffer.rs << 'EOF'
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

pub struct PcmFrame {
    pub utc_ns: u64,
    pub samples: Vec<i16>,
}

pub struct AudioRing {
    buffer: Arc<Mutex<VecDeque<PcmFrame>>>,
    capacity: usize,
    next_seq: u64,
}

impl AudioRing {
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: Arc::new(Mutex::new(VecDeque::with_capacity(capacity))),
            capacity,
            next_seq: 0,
        }
    }
    
    pub fn push(&mut self, utc_ns: u64, samples: Vec<i16>) -> u64 {
        let mut buffer = self.buffer.lock().unwrap();
        
        if buffer.len() >= self.capacity {
            buffer.pop_front();
        }
        
        buffer.push_back(PcmFrame { utc_ns, samples });
        let seq = self.next_seq;
        self.next_seq += 1;
        seq
    }
    
    pub fn stats(&self) -> RingStats {
        let buffer = self.buffer.lock().unwrap();
        RingStats {
            capacity: self.capacity,
            head_seq: self.next_seq.saturating_sub(buffer.len() as u64),
            next_seq: self.next_seq,
            arc_replacements: 0, // simplified
        }
    }
}

#[derive(Debug)]
pub struct RingStats {
    pub capacity: usize,
    pub head_seq: u64,
    pub next_seq: u64,
    pub arc_replacements: u64,
}
EOF

# 2. ALSA Producer mit echtem Audio implementieren
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
}

impl AlsaProducer {
    pub fn new(name: &str, config: &crate::config::ProducerConfig) -> Result<Self> {
        Ok(Self {
            name: name.to_string(),
            running: Arc::new(AtomicBool::new(false)),
            samples_processed: Arc::new(AtomicU64::new(0)),
            config: config.clone(),
            thread_handle: None,
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
        let sample_rate = self.config.sample_rate.unwrap_or(48000);
        let channels = self.config.channels.unwrap_or(2) as u32;
        
        log::info!("ALSA config: device={}, rate={}, channels={}", 
            device, sample_rate, channels);
        
        self.running.store(true, Ordering::SeqCst);
        
        // Thread für Audio-Aufnahme
        let running = self.running.clone();
        let samples_processed = self.samples_processed.clone();
        let name = self.name.clone();
        
        let handle = std::thread::spawn(move || {
            if let Err(e) = Self::run_alsa_capture(
                &device, sample_rate, channels, 
                running.clone(), samples_processed.clone()
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
            connected: true, // Wir versuchen es immer
            samples_processed: self.samples_processed.load(Ordering::Relaxed),
            errors: 0,
        }
    }
}

impl AlsaProducer {
    fn run_alsa_capture(
        device: &str,
        sample_rate: u32,
        channels: u32,
        running: Arc<AtomicBool>,
        samples_processed: Arc<AtomicU64>,
    ) -> Result<()> {
        use alsa::{pcm::{Access, Format, HwParams, PCM}, Direction, ValueOr};
        
        let pcm = PCM::new(device, Direction::Capture, false)
            .with_context(|| format!("Failed to open ALSA device: {}", device))?;
        
        // Hardware-Parameter setzen
        let hwp = HwParams::any(&pcm)?;
        hwp.set_access(Access::RWInterleaved)?;
        hwp.set_format(Format::s16())?;
        hwp.set_channels(channels)?;
        hwp.set_rate(sample_rate, ValueOr::Nearest)?;
        
        // Puffer-Größen
        let period_frames = hwp.set_period_size_near(480, ValueOr::Nearest)?;
        let _buffer_size = hwp.set_buffer_size_near(period_frames * 4)?;
        
        pcm.hw_params(&hwp)?;
        pcm.prepare()?;
        
        log::info!("ALSA capture started: {}Hz, {}ch, period={} frames", 
            sample_rate, channels, period_frames);
        
        let io = pcm.io_i16()?;
        let period_samples = (period_frames as usize) * (channels as usize);
        let mut buffer = vec![0i16; period_samples];
        
        // FIFO für 100ms Chunks (wie im alten Code)
        let target_frames = sample_rate as usize / 10; // 100ms
        let target_samples = target_frames * channels as usize;
        let mut fifo: Vec<i16> = Vec::with_capacity(target_samples * 2);
        
        while running.load(Ordering::Relaxed) {
            match io.readi(&mut buffer) {
                Ok(frames) if frames > 0 => {
                    let samples_read = frames as usize * channels as usize;
                    let slice = &buffer[..samples_read];
                    
                    // Samples zum FIFO hinzufügen
                    fifo.extend_from_slice(slice);
                    samples_processed.fetch_add(samples_read as u64, Ordering::Relaxed);
                    
                    // 100ms-Chunks verarbeiten
                    while fifo.len() >= target_samples {
                        let chunk: Vec<i16> = fifo.drain(..target_samples).collect();
                        
                        // Hier könnten wir den RingBuffer füllen
                        // Für jetzt einfach loggen
                        static mut LAST_LOG: u64 = 0;
                        unsafe {
                            let now = Self::utc_ns_now();
                            if now - LAST_LOG >= 5_000_000_000 { // 5 Sekunden
                                log::debug!("ALSA captured chunk: {} samples", chunk.len());
                                LAST_LOG = now;
                            }
                        }
                    }
                }
                Ok(_) => {
                    // Keine Daten, kurz warten
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(e) => {
                    log::warn!("ALSA read error: {}", e);
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }
        
        log::info!("ALSA capture stopped");
        Ok(())
    }
}
EOF

# 3. ALSA Modul erweitern
cat > src/producers/alsa/mod.rs << 'EOF'
mod scanner;
mod producer;

pub use scanner::AlsaDeviceScanner;
pub use producer::AlsaProducer;
EOF

# 4. RingBuffer ins Core-Modul einbinden
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
pub mod ringbuffer;

pub use device_scanner::*;
pub use ringbuffer::*;

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

# 5. Test-Konfiguration erstellen
cat > config.toml << 'EOF'
node_name = "studio-node"

[producers.mic1]
producer_type = "alsa"
enabled = true
device = "default"
channels = 2
sample_rate = 48000

[producers.background_music]
producer_type = "file"
enabled = true
path = "background.wav"
loop_audio = true
EOF

# 6. Build und Test
cargo build

if [ $? -eq 0 ]; then
    echo "Build erfolgreich! Teste mit:"
    echo "1. Discovery: cargo run -- --discover"
    echo "2. Normal: cargo run"
    echo "3. ALSA Producer testen: cargo run -- --discover 2>&1 | grep -A5 Input"
else
    echo "Build fehlgeschlagen"
fi
