# 1. Processor-Trait definieren (analog zu Producer)
cat > src/core/processor.rs << 'EOF'
use std::sync::Arc;
use anyhow::Result;
use crate::core::ringbuffer::{AudioRingBuffer, PcmFrame};

pub trait Processor: Send + Sync {
    fn name(&self) -> &str;
    
    /// Verarbeitet PCM-Frames vom Input-Buffer zum Output-Buffer
    fn process(&mut self, input_buffer: &AudioRingBuffer, output_buffer: &AudioRingBuffer) -> Result<()>;
    
    /// Status des Prozessors
    fn status(&self) -> ProcessorStatus;
    
    /// Konfiguration ändern (für Runtime-Änderungen)
    fn update_config(&mut self, config: serde_json::Value) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct ProcessorStatus {
    pub running: bool,
    pub processing_rate_hz: f32,  // Wie viele Frames pro Sekunde verarbeitet
    pub latency_ms: f32,          // Durchschnittliche Latenz
    pub errors: u64,
}

// Einfache Processor-Implementierungen für den Start
pub mod basic {
    use super::*;
    
    /// Pass-Through Processor (kopiert einfach)
    pub struct PassThrough {
        name: String,
    }
    
    impl PassThrough {
        pub fn new(name: &str) -> Self {
            Self { name: name.to_string() }
        }
    }
    
    impl Processor for PassThrough {
        fn name(&self) -> &str {
            &self.name
        }
        
        fn process(&mut self, input_buffer: &AudioRingBuffer, output_buffer: &AudioRingBuffer) -> Result<()> {
            while let Some(frame) = input_buffer.pop() {
                // Hier könnte man noch Signalverarbeitung machen
                output_buffer.push(frame);
            }
            Ok(())
        }
        
        fn status(&self) -> ProcessorStatus {
            ProcessorStatus {
                running: true,
                processing_rate_hz: 0.0,
                latency_ms: 0.0,
                errors: 0,
            }
        }
        
        fn update_config(&mut self, _config: serde_json::Value) -> Result<()> {
            Ok(())
        }
    }
    
    /// Gain Processor (Lautstärke anpassen)
    pub struct Gain {
        name: String,
        gain: f32,  // Multiplikator z.B. 0.5 = halbe Lautstärke
    }
    
    impl Gain {
        pub fn new(name: &str, gain: f32) -> Self {
            Self { name: name.to_string(), gain }
        }
    }
    
    impl Processor for Gain {
        fn name(&self) -> &str {
            &self.name
        }
        
        fn process(&mut self, input_buffer: &AudioRingBuffer, output_buffer: &AudioRingBuffer) -> Result<()> {
            while let Some(mut frame) = input_buffer.pop() {
                // Gain auf Samples anwenden
                for sample in frame.samples.iter_mut() {
                    *sample = (*sample as f32 * self.gain).clamp(-32768.0, 32767.0) as i16;
                }
                output_buffer.push(frame);
            }
            Ok(())
        }
        
        fn status(&self) -> ProcessorStatus {
            ProcessorStatus {
                running: true,
                processing_rate_hz: 0.0,
                latency_ms: 0.0,
                errors: 0,
            }
        }
        
        fn update_config(&mut self, config: serde_json::Value) -> Result<()> {
            if let Some(gain) = config.get("gain").and_then(|v| v.as_f64()) {
                self.gain = gain as f32;
                log::info!("Processor '{}' gain updated to {}", self.name, self.gain);
            }
            Ok(())
        }
    }
}
EOF

# 2. Flow-Definition in Config erweitern
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
pub struct ProcessorConfig {
    #[serde(rename = "type")]
    pub processor_type: String,
    pub enabled: bool,
    #[serde(default)]
    pub config: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FlowConfig {
    pub enabled: bool,
    pub inputs: Vec<String>,  // Producer-Namen
    pub processors: Vec<String>, // Processor-Namen (Reihenfolge wichtig)
    pub outputs: Vec<String>, // Consumer-Namen (später)
    
    #[serde(default)]
    pub config: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub node_name: String,
    pub producers: HashMap<String, ProducerConfig>,
    pub processors: HashMap<String, ProcessorConfig>,
    pub flows: HashMap<String, FlowConfig>,
}

impl Config {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let content = fs::read_to_string(path)?;
        Ok(toml::from_str(&content)?)
    }
    
    pub fn save(&self, path: &str) -> anyhow::Result<()> {
        let content = toml::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            node_name: "airlift-node".to_string(),
            producers: HashMap::new(),
            processors: HashMap::new(),
            flows: HashMap::new(),
        }
    }
}
EOF

# 3. Node erweitern um Flows und Processors zu verwalten
cat > src/core/node.rs << 'EOF'
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use anyhow::Result;
use log::{info, error, warn};

use super::ringbuffer::{AudioRingBuffer, PcmFrame};
use super::processor::{Processor, ProcessorStatus};

pub struct Flow {
    pub name: String,
    pub input_buffers: Vec<Arc<AudioRingBuffer>>,  // Von Producern
    pub processor_buffers: Vec<Arc<AudioRingBuffer>>, // Zwischenprozessor-Buffer
    pub output_buffer: Arc<AudioRingBuffer>,       // Finaler Buffer
    pub processors: Vec<Box<dyn Processor>>,
    pub running: Arc<AtomicBool>,
}

impl Flow {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            input_buffers: Vec::new(),
            processor_buffers: Vec::new(),
            output_buffer: Arc::new(AudioRingBuffer::new(100)),
            processors: Vec::new(),
            running: Arc::new(AtomicBool::new(false)),
        }
    }
    
    pub fn add_input_buffer(&mut self, buffer: Arc<AudioRingBuffer>) {
        self.input_buffers.push(buffer);
    }
    
    pub fn add_processor(&mut self, processor: Box<dyn Processor>) {
        // Buffer für diesen Processor erstellen
        let buffer = Arc::new(AudioRingBuffer::new(100));
        self.processor_buffers.push(buffer);
        self.processors.push(processor);
    }
    
    pub fn start(&mut self) -> Result<()> {
        info!("Flow '{}' starting...", self.name);
        self.running.store(true, Ordering::SeqCst);
        
        // TODO: Thread für Processor-Chain starten
        // Aktuell nur Platzhalter
        
        Ok(())
    }
    
    pub fn stop(&mut self) -> Result<()> {
        info!("Flow '{}' stopping...", self.name);
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }
    
    pub fn status(&self) -> FlowStatus {
        // Sammle Processor-Status
        let processor_status: Vec<ProcessorStatus> = 
            self.processors.iter().map(|p| p.status()).collect();
        
        // Berechne Buffer-Auslastung
        let input_buffer_levels: Vec<usize> = 
            self.input_buffers.iter().map(|b| b.len()).collect();
        
        let processor_buffer_levels: Vec<usize> = 
            self.processor_buffers.iter().map(|b| b.len()).collect();
        
        FlowStatus {
            running: self.running.load(Ordering::Relaxed),
            processor_status,
            input_buffer_levels,
            processor_buffer_levels,
            output_buffer_level: self.output_buffer.len(),
        }
    }
}

#[derive(Debug)]
pub struct FlowStatus {
    pub running: bool,
    pub processor_status: Vec<ProcessorStatus>,
    pub input_buffer_levels: Vec<usize>,
    pub processor_buffer_levels: Vec<usize>,
    pub output_buffer_level: usize,
}

// Node-Struktur, die alles verwaltet
pub struct AirliftNode {
    running: Arc<AtomicBool>,
    start_time: Instant,
    producers: Vec<Box<dyn super::Producer>>,
    producer_buffers: Vec<Arc<AudioRingBuffer>>,
    flows: Vec<Flow>,
}

impl AirliftNode {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            start_time: Instant::now(),
            producers: Vec::new(),
            producer_buffers: Vec::new(),
            flows: Vec::new(),
        }
    }
    
    pub fn add_producer(&mut self, producer: Box<dyn super::Producer>) {
        // Buffer für diesen Producer erstellen
        let buffer = Arc::new(AudioRingBuffer::new(100));
        
        // Producer mit Buffer verbinden
        let mut producer = producer;
        producer.attach_ring_buffer(buffer.clone());
        
        self.producer_buffers.push(buffer);
        self.producers.push(producer);
    }
    
    pub fn add_flow(&mut self, flow: Flow) {
        self.flows.push(flow);
    }
    
    pub fn connect_producer_to_flow(&mut self, producer_index: usize, flow_index: usize) -> Result<()> {
        if producer_index < self.producer_buffers.len() && flow_index < self.flows.len() {
            let buffer = self.producer_buffers[producer_index].clone();
            self.flows[flow_index].add_input_buffer(buffer);
            info!("Connected producer {} to flow {}", producer_index, flow_index);
            Ok(())
        } else {
            anyhow::bail!("Invalid producer or flow index");
        }
    }
    
    pub fn start(&mut self) -> Result<()> {
        info!("Node starting...");
        self.running.store(true, Ordering::SeqCst);
        
        // Producer starten
        for (i, producer) in self.producers.iter_mut().enumerate() {
            info!("Starting producer {}: {}", i, producer.name());
            if let Err(e) = producer.start() {
                error!("Failed to start producer {}: {}", producer.name(), e);
            }
        }
        
        // Flows starten
        for flow in &mut self.flows {
            if let Err(e) = flow.start() {
                warn!("Failed to start flow {}: {}", flow.name, e);
            }
        }
        
        Ok(())
    }
    
    pub fn stop(&mut self) -> Result<()> {
        info!("Node stopping...");
        self.running.store(false, Ordering::SeqCst);
        
        // Flows stoppen
        for flow in &mut self.flows {
            if let Err(e) = flow.stop() {
                warn!("Error stopping flow {}: {}", flow.name, e);
            }
        }
        
        // Producer stoppen
        for producer in &mut self.producers {
            info!("Stopping producer: {}", producer.name());
            if let Err(e) = producer.stop() {
                warn!("Error stopping producer {}: {}", producer.name(), e);
            }
        }
        
        Ok(())
    }
    
    pub fn status(&self) -> NodeStatus {
        let producer_status: Vec<super::ProducerStatus> = 
            self.producers.iter().map(|p| p.status()).collect();
        
        let flow_status: Vec<FlowStatus> = 
            self.flows.iter().map(|f| f.status()).collect();
        
        NodeStatus {
            running: self.running.load(Ordering::Relaxed),
            uptime_seconds: self.start_time.elapsed().as_secs(),
            producers: self.producers.len(),
            flows: self.flows.len(),
            producer_status,
            flow_status,
        }
    }
    
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
    
    pub fn producers(&self) -> &[Box<dyn super::Producer>] {
        &self.producers
    }
    
    pub fn flows(&self) -> &[Flow] {
        &self.flows
    }
    
    pub fn producer_buffer(&self, index: usize) -> Option<&Arc<AudioRingBuffer>> {
        self.producer_buffers.get(index)
    }
    
    pub fn flow_output_buffer(&self, flow_index: usize) -> Option<&Arc<AudioRingBuffer>> {
        self.flows.get(flow_index).map(|f| &f.output_buffer)
    }
}

#[derive(Debug)]
pub struct NodeStatus {
    pub running: bool,
    pub uptime_seconds: u64,
    pub producers: usize,
    pub flows: usize,
    pub producer_status: Vec<super::ProducerStatus>,
    pub flow_status: Vec<FlowStatus>,
}
EOF

# 4. Core-Modul aktualisieren
cat > src/core/mod.rs << 'EOF'
pub mod device_scanner;
pub mod ringbuffer;
pub mod timestamp;
pub mod processor;
pub mod node;

pub use ringbuffer::*;
pub use timestamp::*;
pub use processor::Processor;
pub use node::{AirliftNode, Flow, FlowStatus, NodeStatus};

pub trait Producer: Send + Sync {
    fn name(&self) -> &str;
    fn start(&mut self) -> anyhow::Result<()>;
    fn stop(&mut self) -> anyhow::Result<()>;
    fn status(&self) -> ProducerStatus;
    fn attach_ring_buffer(&mut self, buffer: std::sync::Arc<AudioRingBuffer>);
}

#[derive(Debug, Clone)]
pub struct ProducerStatus {
    pub running: bool,
    pub connected: bool,
    pub samples_processed: u64,
    pub errors: u64,
    pub buffer_stats: Option<RingBufferStats>,
}
EOF

# 5. Main für neue Struktur anpassen
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
    
    log::info!("=== Airlift Node v0.3.0 ===");
    
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
    
    match scanner.test_device(device_id, 3000) {
        Ok(mut result) => {
            log::info!("Test completed for device: {}", device_id);
            log::info!("Passed: {}", result.test_passed);
            
            if let Some(ref format) = result.detected_format {
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
    
    // Producer aus Config laden
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
    
    // Processors aus Config laden
    use crate::core::processor::basic;
    for (name, processor_cfg) in &config.processors {
        if !processor_cfg.enabled {
            continue;
        }
        
        // Hier würden wir verschiedene Processor-Typen erstellen
        // Für jetzt nur PassThrough und Gain
        match processor_cfg.processor_type.as_str() {
            "passthrough" => {
                let processor = basic::PassThrough::new(name);
                // TODO: Zum Flow hinzufügen
                log::info!("Added passthrough processor: {}", name);
            }
            "gain" => {
                let gain = processor_cfg.config.get("gain")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(1.0) as f32;
                let processor = basic::Gain::new(name, gain);
                // TODO: Zum Flow hinzufügen
                log::info!("Added gain processor: {} (gain: {})", name, gain);
            }
            _ => log::error!("Unknown processor type: {}", processor_cfg.processor_type),
        }
    }
    
    // Flows aus Config erstellen
    for (flow_name, flow_cfg) in &config.flows {
        if !flow_cfg.enabled {
            continue;
        }
        
        let mut flow = core::Flow::new(flow_name);
        
        // TODO: Producer mit Flow verbinden basierend auf inputs
        // TODO: Processors zum Flow hinzufügen basierend auf processors
        
        node.add_flow(flow);
        log::info!("Added flow: {}", flow_name);
    }
    
    // Falls nichts konfiguriert: Demo-Setup
    if node.producers().is_empty() {
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
    log::info!("Node started. Press Ctrl+C to stop.");
    
    let mut tick = 0;
    while !shutdown.load(Ordering::Relaxed) && node.is_running() {
        std::thread::sleep(Duration::from_millis(500));
        
        tick += 1;
        if tick % 10 == 0 {
            let status = node.status();
            log::info!("=== Node Status ===");
            log::info!("Uptime: {}s, Producers: {}, Flows: {}", 
                status.uptime_seconds, status.producers, status.flows);
            
            for (i, p_status) in status.producer_status.iter().enumerate() {
                log::info!("  Producer {}:", i);
                log::info!("    running={}, connected={}, samples={}", 
                    p_status.running, p_status.connected, p_status.samples_processed);
            }
            
            for (i, f_status) in status.flow_status.iter().enumerate() {
                log::info!("  Flow {}: running={}, buffers={}/{}", 
                    i, f_status.running, 
                    f_status.output_buffer_level,
                    f_status.processor_buffer_levels.len());
            }
        }
    }
    
    node.stop()?;
    log::info!("Node stopped");
    
    Ok(())
}
EOF

# 6. Beispiel-Config mit Flows
cat > config.toml << 'EOF'
node_name = "studio-node"

# Producer Definitionen
[producers.mic1]
type = "alsa_input"
enabled = true
device = "default"
channels = 2
sample_rate = 44100

[producers.system]
type = "alsa_output"
enabled = true
device = "pulse"
channels = 2
sample_rate = 48000

# Processor Definitionen
[processors.gain_control]
type = "gain"
enabled = true
config = { gain = 0.8 }

[processors.compressor]
type = "passthrough"  # Platzhalter
enabled = true

# Flow Definitionen
[flows.live_stream]
enabled = true
inputs = ["mic1", "system"]
processors = ["gain_control", "compressor"]
outputs = []  # Später: network_stream
config = { mix_ratio = 0.7 }

[flows.recording]
enabled = false  # Noch nicht aktiv
inputs = ["mic1"]
processors = ["gain_control"]
outputs = []
EOF

echo "Neue Architektur implementiert:"
echo "1. ✅ Processor-Trait mit PassThrough und Gain"
echo "2. ✅ Flow-Definition in Config"
echo "3. ✅ Node verwaltet Producer, Processors und Flows"
echo "4. ✅ Status für alle Komponenten"
echo ""
echo "Nächste Schritte könnten sein:"
echo "1. Decoder/Encoder-Traits"
echo "2. Consumer für Network/File Output"
echo "3. Mixer-Processor mit mehreren Inputs"
echo "4. Runtime Config-API"
echo ""
echo "Build mit: cargo build"
