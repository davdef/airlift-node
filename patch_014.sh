# 1. Config Hot-Reload System
cat > src/core/config_manager.rs << 'EOF'
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use std::fs;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use anyhow::Result;
use crate::config::Config;

pub struct ConfigManager {
    config: Arc<Mutex<Config>>,
    last_modified: Arc<Mutex<SystemTime>>,
    config_path: String,
}

impl ConfigManager {
    pub fn new(path: &str) -> Result<Self> {
        let config = Config::load(path)?;
        let metadata = fs::metadata(path)?;
        let modified = metadata.modified()?;
        
        Ok(Self {
            config: Arc::new(Mutex::new(config)),
            last_modified: Arc::new(Mutex::new(modified)),
            config_path: path.to_string(),
        })
    }
    
    pub fn get_config(&self) -> Config {
        let config = self.config.lock().unwrap();
        config.clone()
    }
    
    pub fn start_watcher(&self) -> Result<()> {
        let config_clone = self.config.clone();
        let modified_clone = self.last_modified.clone();
        let path_clone = self.config_path.clone();
        
        std::thread::spawn(move || {
            let mut watcher: RecommendedWatcher = Watcher::new_immediate(move |res| {
                match res {
                    Ok(event) => {
                        if event.paths.iter().any(|p| p.to_string_lossy().contains(&path_clone)) {
                            match Self::try_reload_config(&path_clone, &config_clone, &modified_clone) {
                                Ok(true) => log::info!("Config reloaded successfully"),
                                Ok(false) => log::debug!("Config unchanged"),
                                Err(e) => log::error!("Failed to reload config: {}", e),
                            }
                        }
                    }
                    Err(e) => log::error!("Config watch error: {}", e),
                }
            }).expect("Failed to create watcher");
            
            watcher.watch(&path_clone, RecursiveMode::NonRecursive).unwrap();
            
            // Keep thread alive
            loop {
                std::thread::sleep(Duration::from_secs(1));
            }
        });
        
        Ok(())
    }
    
    fn try_reload_config(
        path: &str,
        config: &Arc<Mutex<Config>>,
        last_modified: &Arc<Mutex<SystemTime>>,
    ) -> Result<bool> {
        let metadata = fs::metadata(path)?;
        let modified = metadata.modified()?;
        
        let mut last_mod = last_modified.lock().unwrap();
        if &modified > &*last_mod {
            *last_mod = modified;
            
            match Config::load(path) {
                Ok(new_config) => {
                    let mut config_lock = config.lock().unwrap();
                    *config_lock = new_config;
                    Ok(true)
                }
                Err(e) => {
                    log::error!("Failed to parse new config: {}", e);
                    Ok(false)
                }
            }
        } else {
            Ok(false)
        }
    }
}
EOF

# 2. Erweitertes Device Testing mit Signal-Analyse
cat > src/core/signal_analyzer.rs << 'EOF'
use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub struct SignalAnalysis {
    pub rms_levels: Vec<f32>,        // RMS pro Kanal [-1.0..1.0]
    pub peak_levels: Vec<f32>,       // Peak pro Kanal [-1.0..1.0]
    pub clipping: bool,              // Clipping erkannt?
    pub dc_offset: Vec<f32>,         // DC-Offset pro Kanal
    pub frequency_hint: Option<f32>, // Dominante Frequenz (Hz)
    pub noise_floor: f32,            // Grundrauschen
    pub channel_correlation: f32,    // Korrelation zwischen Kanälen (bei Stereo)
}

pub struct SignalAnalyzer {
    sample_rate: u32,
    channels: usize,
    buffer: VecDeque<i16>,
    max_buffer_size: usize,
}

impl SignalAnalyzer {
    pub fn new(sample_rate: u32, channels: usize) -> Self {
        // 1 Sekunde Buffer für Frequenzanalyse
        let max_buffer_size = sample_rate as usize * channels;
        
        Self {
            sample_rate,
            channels,
            buffer: VecDeque::with_capacity(max_buffer_size),
            max_buffer_size,
        }
    }
    
    pub fn feed_samples(&mut self, samples: &[i16]) {
        for &sample in samples {
            self.buffer.push_back(sample);
            if self.buffer.len() > self.max_buffer_size {
                self.buffer.pop_front();
            }
        }
    }
    
    pub fn analyze(&self) -> SignalAnalysis {
        let samples: Vec<i16> = self.buffer.iter().copied().collect();
        
        if samples.is_empty() {
            return SignalAnalysis {
                rms_levels: vec![0.0; self.channels],
                peak_levels: vec![0.0; self.channels],
                clipping: false,
                dc_offset: vec![0.0; self.channels],
                frequency_hint: None,
                noise_floor: 0.0,
                channel_correlation: 0.0,
            };
        }
        
        // Separiere Kanäle
        let mut channel_samples: Vec<Vec<f32>> = (0..self.channels)
            .map(|_| Vec::new())
            .collect();
        
        for (i, &sample) in samples.iter().enumerate() {
            let channel = i % self.channels;
            channel_samples[channel].push(sample as f32 / 32768.0);
        }
        
        // Analysiere jeden Kanal
        let rms_levels: Vec<f32> = channel_samples.iter()
            .map(|ch| Self::calculate_rms(ch))
            .collect();
        
        let peak_levels: Vec<f32> = channel_samples.iter()
            .map(|ch| Self::calculate_peak(ch))
            .collect();
        
        let clipping = peak_levels.iter().any(|&p| p >= 0.99);
        
        let dc_offset: Vec<f32> = channel_samples.iter()
            .map(|ch| Self::calculate_dc_offset(ch))
            .collect();
        
        let frequency_hint = if self.buffer.len() >= 1024 {
            Self::estimate_frequency(&channel_samples[0], self.sample_rate)
        } else {
            None
        };
        
        let noise_floor = Self::estimate_noise_floor(&channel_samples[0]);
        
        let channel_correlation = if self.channels >= 2 {
            Self::calculate_correlation(&channel_samples[0], &channel_samples[1])
        } else {
            0.0
        };
        
        SignalAnalysis {
            rms_levels,
            peak_levels,
            clipping,
            dc_offset,
            frequency_hint,
            noise_floor,
            channel_correlation,
        }
    }
    
    fn calculate_rms(samples: &[f32]) -> f32 {
        if samples.is_empty() { return 0.0; }
        let sum_squares: f32 = samples.iter().map(|&s| s * s).sum();
        (sum_squares / samples.len() as f32).sqrt()
    }
    
    fn calculate_peak(samples: &[f32]) -> f32 {
        samples.iter()
            .map(|&s| s.abs())
            .fold(0.0, |a, b| a.max(b))
    }
    
    fn calculate_dc_offset(samples: &[f32]) -> f32 {
        if samples.is_empty() { return 0.0; }
        samples.iter().sum::<f32>() / samples.len() as f32
    }
    
    fn estimate_frequency(samples: &[f32], sample_rate: u32) -> Option<f32> {
        // Einfache Zero-Crossing Frequenzschätzung
        if samples.len() < 2 {
            return None;
        }
        
        let mut zero_crossings = 0;
        for i in 1..samples.len() {
            if samples[i-1] * samples[i] < 0.0 {
                zero_crossings += 1;
            }
        }
        
        if zero_crossings > 0 {
            let duration = samples.len() as f32 / sample_rate as f32;
            Some(zero_crossings as f32 / (2.0 * duration))
        } else {
            None
        }
    }
    
    fn estimate_noise_floor(samples: &[f32]) -> f32 {
        // RMS der letzten 10% der Samples als Rauschen
        if samples.len() < 10 { return 0.0; }
        let start = samples.len() * 9 / 10;
        let noise_samples = &samples[start..];
        Self::calculate_rms(noise_samples)
    }
    
    fn calculate_correlation(ch1: &[f32], ch2: &[f32]) -> f32 {
        let n = ch1.len().min(ch2.len());
        if n < 2 { return 0.0; }
        
        let mean1: f32 = ch1[..n].iter().sum::<f32>() / n as f32;
        let mean2: f32 = ch2[..n].iter().sum::<f32>() / n as f32;
        
        let mut numerator = 0.0;
        let mut denom1 = 0.0;
        let mut denom2 = 0.0;
        
        for i in 0..n {
            let diff1 = ch1[i] - mean1;
            let diff2 = ch2[i] - mean2;
            numerator += diff1 * diff2;
            denom1 += diff1 * diff1;
            denom2 += diff2 * diff2;
        }
        
        if denom1 > 0.0 && denom2 > 0.0 {
            numerator / (denom1.sqrt() * denom2.sqrt())
        } else {
            0.0
        }
    }
}
EOF

# 3. Output-Capture Support (PulseAudio oder ALSA loopback)
cat > src/producers/alsa/output_capture.rs << 'EOF'
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
use anyhow::{Context, Result};

pub struct AlsaOutputCapture {
    name: String,
    running: Arc<AtomicBool>,
    samples_processed: Arc<AtomicU64>,
    config: crate::config::ProducerConfig,
    thread_handle: Option<std::thread::JoinHandle<()>>,
    ring_buffer: Option<Arc<crate::core::AudioRingBuffer>>,
}

impl AlsaOutputCapture {
    pub fn new(name: &str, config: &crate::config::ProducerConfig) -> Result<Self> {
        Ok(Self {
            name: name.to_string(),
            running: Arc::new(AtomicBool::new(false)),
            samples_processed: Arc::new(AtomicU64::new(0)),
            config: config.clone(),
            thread_handle: None,
            ring_buffer: None,
        })
    }
}

impl crate::core::Producer for AlsaOutputCapture {
    fn name(&self) -> &str {
        &self.name
    }
    
    fn start(&mut self) -> Result<()> {
        if self.running.load(Ordering::Relaxed) {
            return Ok(());
        }
        
        log::info!("ALSA Output Capture '{}' starting...", self.name);
        
        // Für Output-Capture: loopback Device verwenden
        // Oder PulseAudio Monitor Source
        let device = self.config.device.clone()
            .unwrap_or_else(|| {
                // Versuche verschiedene Output-Capture Devices
                "pulse".to_string()  // PulseAudio Monitor
                // "hw:Loopback,1,0"  // ALSA Loopback wenn konfiguriert
            });
        
        let sample_rate = self.config.sample_rate.unwrap_or(48000);
        let channels = self.config.channels.unwrap_or(2) as u32;
        
        log::info!("Output Capture config: device={}, rate={}, channels={}", 
            device, sample_rate, channels);
        
        self.running.store(true, Ordering::SeqCst);
        
        let running = self.running.clone();
        let samples_processed = self.samples_processed.clone();
        let name = self.name.clone();
        let ring_buffer = self.ring_buffer.clone();
        
        let handle = std::thread::spawn(move || {
            if let Err(e) = Self::capture_output(
                &device, sample_rate, channels, 
                running.clone(), samples_processed.clone(),
                ring_buffer,
            ) {
                log::error!("Output Capture '{}' error: {}", name, e);
            }
            log::info!("Output Capture '{}' thread stopped", name);
        });
        
        self.thread_handle = Some(handle);
        Ok(())
    }
    
    fn stop(&mut self) -> Result<()> {
        log::info!("Output Capture '{}' stopping...", self.name);
        self.running.store(false, Ordering::SeqCst);
        
        if let Some(handle) = self.thread_handle.take() {
            if let Err(e) = handle.join() {
                log::error!("Failed to join output capture thread: {:?}", e);
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

impl AlsaOutputCapture {
    fn capture_output(
        device: &str,
        sample_rate: u32,
        channels: u32,
        running: Arc<AtomicBool>,
        samples_processed: Arc<AtomicU64>,
        ring_buffer: Option<Arc<crate::core::AudioRingBuffer>>,
    ) -> Result<()> {
        use alsa::{pcm::{Access, Format, HwParams, PCM}, Direction, ValueOr};
        
        // Output-Capture ist auch Capture, aber von einem speziellen Device
        let pcm = PCM::new(device, Direction::Capture, false)
            .with_context(|| format!("Failed to open output device: {}", device))?;
        
        let hwp = HwParams::any(&pcm)?;
        hwp.set_access(Access::RWInterleaved)?;
        
        let format_result = hwp.set_format(Format::s16())
            .or_else(|_| hwp.set_format(Format::s32()))
            .or_else(|_| hwp.set_format(Format::float()));
        
        if let Err(e) = format_result {
            log::warn!("No supported format for output capture: {}", e);
            // Fallback zu Demo-Modus
            return Self::capture_demo(sample_rate, channels, running, samples_processed, ring_buffer);
        }
        
        hwp.set_channels(channels)?;
        hwp.set_rate(sample_rate, ValueOr::Nearest)?;
        
        let period_frames = hwp.set_period_size_near(480, ValueOr::Nearest)?;
        let _buffer_size = hwp.set_buffer_size_near(period_frames * 4)?;
        
        pcm.hw_params(&hwp)?;
        pcm.prepare()?;
        
        log::info!("Output capture started: {}Hz, {}ch", sample_rate, channels);
        
        if let Ok(io) = pcm.io_i16() {
            Self::capture_i16(io, period_frames as usize, channels as usize, 
                sample_rate, running, samples_processed, ring_buffer)?;
        } else {
            log::warn!("i16 capture failed, using demo mode");
            Self::capture_demo(sample_rate, channels, running, samples_processed, ring_buffer)?;
        }
        
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
        let target_frames = sample_rate as usize / 10;
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
                    
                    while fifo.len() >= target_samples {
                        let chunk_samples: Vec<i16> = fifo.drain(..target_samples).collect();
                        
                        if let Some(rb) = &ring_buffer {
                            let frame = crate::core::PcmFrame {
                                utc_ns: crate::core::utc_ns_now(),
                                samples: chunk_samples.clone(),
                                sample_rate,
                                channels: channels as u8,
                            };
                            rb.push(frame);
                        }
                    }
                }
                Ok(_) => std::thread::sleep(Duration::from_millis(1)),
                Err(e) => {
                    log::warn!("Output capture read error: {}", e);
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
        log::warn!("Output capture demo mode - simulating system audio");
        
        let target_frames = sample_rate as usize / 10;
        let target_samples = target_frames * channels as usize;
        
        let mut tick = 0;
        while running.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(100));
            tick += 1;
            
            if tick % 10 == 0 {
                let chunk_samples = vec![0i16; target_samples];
                samples_processed.fetch_add(target_samples as u64, Ordering::Relaxed);
                
                if let Some(rb) = &ring_buffer {
                    let frame = crate::core::PcmFrame {
                        utc_ns: crate::core::utc_ns_now(),
                        samples: chunk_samples,
                        sample_rate,
                        channels: channels as u8,
                    };
                    rb.push(frame);
                }
            }
        }
        
        Ok(())
    }
}
EOF

# 4. ALSA Modul erweitern
cat > src/producers/alsa/mod.rs << 'EOF'
mod scanner;
pub mod producer;
mod output_capture;

pub use scanner::AlsaDeviceScanner;
pub use producer::AlsaProducer;
pub use output_capture::AlsaOutputCapture;
EOF

# 5. Main für Hot-Reload und Testing erweitern
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
    
    if args.len() > 1 {
        match args[1].as_str() {
            "--discover" => return run_discovery(),
            "--test-device" => {
                if args.len() > 2 {
                    return test_device(&args[2]);
                } else {
                    log::error!("Please specify device ID: cargo run -- --test-device <device_id>");
                    return Ok(());
                }
            }
            _ => {}
        }
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
            
            // Auch detaillierte Info loggen
            for device in &devices {
                log::info!("[{}] {} - {} (max channels: {}, rates: {:?})", 
                    device.id,
                    device.name,
                    match device.device_type {
                        crate::core::device_scanner::DeviceType::Input => "Input",
                        crate::core::device_scanner::DeviceType::Output => "Output",
                        crate::core::device_scanner::DeviceType::Duplex => "Duplex",
                    },
                    device.max_channels,
                    device.supported_rates
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

fn test_device(device_id: &str) -> anyhow::Result<()> {
    log::info!("Testing device: {}", device_id);
    
    use crate::core::device_scanner::DeviceScanner;
    let scanner = producers::alsa::AlsaDeviceScanner;
    
    match scanner.test_device(device_id, 3000) { // 3 Sekunden Test
        Ok(result) => {
            log::info!("Test completed for device: {}", device_id);
            log::info!("Passed: {}", result.test_passed);
            
            if let Some(format) = result.detected_format {
                log::info!("Detected format: {}-bit {} @ {}Hz, {} channel{}",
                    format.bit_depth,
                    match format.sample_type {
                        crate::core::device_scanner::SampleType::SignedInteger => "SInt",
                        crate::core::device_scanner::SampleType::Float => "Float",
                    },
                    format.sample_rate,
                    format.channels,
                    if format.channels > 1 { "s" } else { "" }
                );
            }
            
            if !result.warnings.is_empty() {
                log::warn!("Warnings:");
                for warning in &result.warnings {
                    log::warn!("  - {}", warning);
                }
            }
            
            if !result.errors.is_empty() {
                log::error!("Errors:");
                for error in &result.errors {
                    log::error!("  - {}", error);
                }
            }
            
            // JSON Output für Skripte
            let json = serde_json::to_string_pretty(&result)?;
            println!("{}", json);
        }
        Err(e) => {
            log::error!("Device test failed: {}", e);
            anyhow::bail!("Test failed: {}", e);
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
            "alsa_input" => {
                match producers::alsa::AlsaProducer::new(name, producer_cfg) {
                    Ok(producer) => {
                        node.add_producer(Box::new(producer));
                        log::info!("Added ALSA input producer: {}", name);
                    }
                    Err(e) => {
                        log::error!("Failed to create ALSA producer {}: {}", name, e);
                    }
                }
            }
            "alsa_output" => {
                match producers::alsa::AlsaOutputCapture::new(name, producer_cfg) {
                    Ok(producer) => {
                        node.add_producer(Box::new(producer));
                        log::info!("Added ALSA output capture: {}", name);
                    }
                    Err(e) => {
                        log::error!("Failed to create output capture {}: {}", name, e);
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
    
    // Config Hot-Reload starten
    let config_manager = Arc::new(core::config_manager::ConfigManager::new("config.toml")?);
    config_manager.start_watcher()?;
    log::info!("Config hot-reload enabled. Modify config.toml to see changes.");
    
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
            
            for (i, producer) in node.producers().iter().enumerate() {
                let p_status = producer.status();
                log::info!("  [{:2}] {}:", i, producer.name());
                log::info!("       running={}, connected={}, samples={}", 
                    p_status.running,
                    p_status.connected,
                    p_status.samples_processed
                );
                
                if let Some(stats) = &p_status.buffer_stats {
                    log::info!("       buffer: {}/{} frames, dropped={}", 
                        stats.current_frames, stats.capacity, stats.dropped_frames);
                }
            }
        }
        
        // Optional: Config ändern im laufenden Betrieb
        if tick % 30 == 0 { // Alle 15 Sekunden
            let current_config = config_manager.get_config();
            // Hier könnten wir auf Config-Änderungen reagieren
            // z.B. neue Producer starten/stoppen
        }
    }
    
    node.stop()?;
    log::info!("Node stopped");
    
    Ok(())
}
EOF

# 6. Notify dependency hinzufügen
cat >> Cargo.toml << 'EOF'
notify = "6.0"
EOF

# 7. Beispiel Config mit Output-Capture
cat > config.toml << 'EOF'
node_name = "studio-node"

[producers.microphone]
type = "alsa_input"
enabled = true
device = "default"
channels = 2
sample_rate = 44100

[producers.system_audio]
type = "alsa_output" 
enabled = true
device = "pulse"  # PulseAudio Monitor
# device = "hw:Loopback,1,0"  # ALSA Loopback
channels = 2
sample_rate = 48000

[producers.background]
type = "file"
enabled = true
path = "background.wav"
loop_audio = true
EOF

# 8. Build
echo "Building advanced features..."
cargo build && echo "
=== FEATURES READY ===
1. Device Discovery: cargo run -- --discover
2. Device Testing: cargo run -- --test-device <device_id>
3. Hot-Reload: Ändere config.toml während Node läuft
4. Output-Capture: System-Audio mitschneiden
5. Signal-Analyse: Für zukünftige Qualitätsprüfung
"
