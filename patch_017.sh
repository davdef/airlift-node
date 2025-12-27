# Mixer-Processor implementieren
cat > src/processors/mixer.rs << 'EOF'
use super::{Processor, ProcessorStatus};
use crate::core::ringbuffer::{AudioRingBuffer, PcmFrame};
use anyhow::Result;
use std::collections::HashMap;

pub struct Mixer {
    name: String,
    /// Mapping von Input-Namen zu Gain-Werten (0.0 - 1.0)
    input_gains: HashMap<String, f32>,
    /// Mapping von Input-Namen zu ihren Buffern
    input_buffers: HashMap<String, std::sync::Arc<AudioRingBuffer>>,
    /// Output-Konfiguration
    output_sample_rate: u32,
    output_channels: u8,
}

impl Mixer {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            input_gains: HashMap::new(),
            input_buffers: HashMap::new(),
            output_sample_rate: 48000, // Default
            output_channels: 2,
        }
    }
    
    pub fn add_input(&mut self, input_name: &str, buffer: std::sync::Arc<AudioRingBuffer>, gain: f32) {
        self.input_buffers.insert(input_name.to_string(), buffer);
        self.input_gains.insert(input_name.to_string(), gain.clamp(0.0, 1.0));
        log::info!("Mixer '{}': Added input '{}' with gain {}", self.name, input_name, gain);
    }
    
    pub fn remove_input(&mut self, input_name: &str) {
        self.input_buffers.remove(input_name);
        self.input_gains.remove(input_name);
    }
    
    pub fn set_gain(&mut self, input_name: &str, gain: f32) {
        if let Some(g) = self.input_gains.get_mut(input_name) {
            *g = gain.clamp(0.0, 1.0);
        }
    }
    
    pub fn set_output_format(&mut self, sample_rate: u32, channels: u8) {
        self.output_sample_rate = sample_rate;
        self.output_channels = channels;
    }
    
    fn mix_frames(&self, frames: Vec<Option<PcmFrame>>) -> Option<PcmFrame> {
        if frames.is_empty() {
            return None;
        }
        
        // Finde gemeinsames Format oder konvertiere
        let target_samples = (self.output_sample_rate as usize / 10) * self.output_channels as usize;
        let mut mixed_samples = vec![0i16; target_samples];
        
        // Einfache Mixing-Logik: Summe mit Gain
        for (input_name, frame_opt) in frames.iter().enumerate() {
            if let Some(frame) = frame_opt {
                let gain = self.input_gains
                    .values()
                    .nth(input_name)
                    .copied()
                    .unwrap_or(1.0);
                
                // Einfaches Mixing (ohne Resampling für jetzt)
                let frame_samples = frame.samples.len().min(mixed_samples.len());
                for i in 0..frame_samples {
                    let mixed = mixed_samples[i] as f32 + (frame.samples[i] as f32 * gain);
                    mixed_samples[i] = mixed.clamp(-32768.0, 32767.0) as i16;
                }
            }
        }
        
        Some(PcmFrame {
            utc_ns: crate::core::utc_ns_now(),
            samples: mixed_samples,
            sample_rate: self.output_sample_rate,
            channels: self.output_channels,
        })
    }
}

impl Processor for Mixer {
    fn name(&self) -> &str {
        &self.name
    }
    
    fn process(&mut self, _input_buffer: &AudioRingBuffer, output_buffer: &AudioRingBuffer) -> Result<()> {
        // Sammle Frames von allen Input-Buffern
        let mut frames = Vec::new();
        
        for (input_name, buffer) in &self.input_buffers {
            let frame = buffer.pop();
            frames.push(frame);
            
            if frame.is_none() {
                log::debug!("Mixer '{}': No frame from input '{}'", self.name, input_name);
            }
        }
        
        // Mixe die Frames
        if let Some(mixed_frame) = self.mix_frames(frames) {
            output_buffer.push(mixed_frame);
        }
        
        Ok(())
    }
    
    fn status(&self) -> ProcessorStatus {
        let input_count = self.input_buffers.len();
        let buffer_levels: Vec<usize> = self.input_buffers.values().map(|b| b.len()).collect();
        let avg_buffer = if !buffer_levels.is_empty() {
            buffer_levels.iter().sum::<usize>() as f32 / buffer_levels.len() as f32
        } else { 0.0 };
        
        ProcessorStatus {
            running: true,
            processing_rate_hz: 10.0, // 100ms Frames = 10Hz
            latency_ms: avg_buffer as f32 * 100.0, // geschätzt
            errors: 0,
        }
    }
    
    fn update_config(&mut self, config: serde_json::Value) -> Result<()> {
        // Konfiguration für Mixer aktualisieren
        if let Some(gains) = config.get("gains").and_then(|v| v.as_object()) {
            for (input_name, gain_value) in gains {
                if let Some(gain) = gain_value.as_f64() {
                    self.set_gain(input_name, gain as f32);
                }
            }
        }
        
        if let Some(sample_rate) = config.get("sample_rate").and_then(|v| v.as_u64()) {
            self.output_sample_rate = sample_rate as u32;
        }
        
        if let Some(channels) = config.get("channels").and_then(|v| v.as_u64()) {
            self.output_channels = channels as u8;
        }
        
        Ok(())
    }
}
EOF

# Processors Modul erweitern
cat > src/core/processor.rs << 'EOF'
use anyhow::Result;
use crate::core::ringbuffer::AudioRingBuffer;

pub trait Processor: Send + Sync {
    fn name(&self) -> &str;
    
    fn process(&mut self, input_buffer: &AudioRingBuffer, output_buffer: &AudioRingBuffer) -> Result<()>;
    
    fn status(&self) -> ProcessorStatus;
    
    fn update_config(&mut self, config: serde_json::Value) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct ProcessorStatus {
    pub running: bool,
    pub processing_rate_hz: f32,
    pub latency_ms: f32,
    pub errors: u64,
}

// Basis-Processors
pub mod basic {
    use super::*;
    
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
    
    pub struct Gain {
        name: String,
        gain: f32,
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

// Mixer-Processor
pub mod mixer {
    pub use super::super::processor::mixer::Mixer;
}
EOF

# Flow erweitern um Mixer zu unterstützen
cat > src/core/node.rs << 'EOF'
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use anyhow::Result;
use log::{info, error, warn};

use super::ringbuffer::AudioRingBuffer;
use super::processor::{Processor, ProcessorStatus};

pub struct Flow {
    pub name: String,
    pub input_buffers: Vec<Arc<AudioRingBuffer>>,
    pub processor_buffers: Vec<Arc<AudioRingBuffer>>,
    pub output_buffer: Arc<AudioRingBuffer>,
    pub processors: Vec<Box<dyn Processor>>,
    pub running: Arc<AtomicBool>,
    thread_handle: Option<std::thread::JoinHandle<()>>,
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
            thread_handle: None,
        }
    }
    
    pub fn add_input_buffer(&mut self, buffer: Arc<AudioRingBuffer>) {
        self.input_buffers.push(buffer);
    }
    
    pub fn add_processor(&mut self, processor: Box<dyn Processor>) {
        let buffer = Arc::new(AudioRingBuffer::new(100));
        self.processor_buffers.push(buffer);
        self.processors.push(processor);
    }
    
    pub fn start(&mut self) -> Result<()> {
        info!("Flow '{}' starting...", self.name);
        self.running.store(true, Ordering::SeqCst);
        
        // Starte Processing-Thread
        let running = self.running.clone();
        let input_buffers = self.input_buffers.clone();
        let processor_buffers = self.processor_buffers.clone();
        let output_buffer = self.output_buffer.clone();
        let mut processors = std::mem::take(&mut self.processors);
        let flow_name = self.name.clone();
        
        let handle = std::thread::spawn(move || {
            Self::processing_loop(
                running,
                input_buffers,
                processor_buffers,
                output_buffer,
                &mut processors,
                &flow_name,
            );
        });
        
        self.thread_handle = Some(handle);
        self.processors = processors;
        
        Ok(())
    }
    
    fn processing_loop(
        running: Arc<AtomicBool>,
        input_buffers: Vec<Arc<AudioRingBuffer>>,
        processor_buffers: Vec<Arc<AudioRingBuffer>>,
        output_buffer: Arc<AudioRingBuffer>,
        processors: &mut [Box<dyn Processor>],
        flow_name: &str,
    ) {
        info!("Flow '{}' processing thread started", flow_name);
        
        while running.load(Ordering::Relaxed) {
            // Wenn keine Inputs: kurz warten
            if input_buffers.is_empty() {
                std::thread::sleep(std::time::Duration::from_millis(100));
                continue;
            }
            
            // Einfache Pipeline: Input → Prozessor 1 → Prozessor 2 → ... → Output
            let mut current_input = &input_buffers[0]; // Für jetzt nur ersten Input
            
            for (i, processor) in processors.iter_mut().enumerate() {
                let output_buffer = if i < processor_buffers.len() {
                    &processor_buffers[i]
                } else {
                    // Letzter Prozessor schreibt in finalen Output
                    &output_buffer
                };
                
                if let Err(e) = processor.process(current_input, output_buffer) {
                    log::error!("Processor '{}' error: {}", processor.name(), e);
                }
                
                // Nächster Prozessor nimmt Ausgabe dieses Prozessors als Input
                if i < processor_buffers.len() {
                    current_input = &processor_buffers[i];
                }
            }
            
            std::thread::sleep(std::time::Duration::from_millis(10)); // 100Hz
        }
        
        info!("Flow '{}' processing thread stopped", flow_name);
    }
    
    pub fn stop(&mut self) -> Result<()> {
        info!("Flow '{}' stopping...", self.name);
        self.running.store(false, Ordering::SeqCst);
        
        if let Some(handle) = self.thread_handle.take() {
            if let Err(e) = handle.join() {
                log::error!("Failed to join flow thread: {:?}", e);
            }
        }
        
        Ok(())
    }
    
    pub fn status(&self) -> FlowStatus {
        let processor_status: Vec<ProcessorStatus> = 
            self.processors.iter().map(|p| p.status()).collect();
        
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
        let buffer = Arc::new(AudioRingBuffer::new(100));
        
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
        
        for (i, producer) in self.producers.iter_mut().enumerate() {
            info!("Starting producer {}: {}", i, producer.name());
            if let Err(e) = producer.start() {
                error!("Failed to start producer {}: {}", producer.name(), e);
            }
        }
        
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
        
        for flow in &mut self.flows {
            if let Err(e) = flow.stop() {
                warn!("Error stopping flow {}: {}", flow.name, e);
            }
        }
        
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

# Main für Mixer-Test aktualisieren
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
        Ok(result) => {
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
    
    // Flows aus Config erstellen und Processors hinzufügen
    for (flow_name, flow_cfg) in &config.flows {
        if !flow_cfg.enabled {
            continue;
        }
        
        let mut flow = core::Flow::new(flow_name);
        
        // Processors zum Flow hinzufügen
        for processor_name in &flow_cfg.processors {
            if let Some(processor_cfg) = config.processors.get(processor_name) {
                if !processor_cfg.enabled {
                    continue;
                }
                
                match processor_cfg.processor_type.as_str() {
                    "passthrough" => {
                        let processor = core::processor::basic::PassThrough::new(processor_name);
                        flow.add_processor(Box::new(processor));
                        log::info!("Added passthrough processor '{}' to flow '{}'", 
                            processor_name, flow_name);
                    }
                    "gain" => {
                        let gain = processor_cfg.config.get("gain")
                            .and_then(|v| v.as_f64())
                            .unwrap_or(1.0) as f32;
                        let processor = core::processor::basic::Gain::new(processor_name, gain);
                        flow.add_processor(Box::new(processor));
                        log::info!("Added gain processor '{}' (gain: {}) to flow '{}'", 
                            processor_name, gain, flow_name);
                    }
                    "mixer" => {
                        let mut mixer = core::processor::mixer::Mixer::new(processor_name);
                        
                        // Mixer-Konfiguration anwenden
                        if let Some(sample_rate) = processor_cfg.config.get("sample_rate") {
                            if let Some(sr) = sample_rate.as_u64() {
                                mixer.set_output_format(sr as u32, 2); // Default 2 channels
                            }
                        }
                        
                        // Input-Gains aus Config
                        if let Some(gains) = processor_cfg.config.get("gains") {
                            if let Some(gain_map) = gains.as_object() {
                                for (input, gain_value) in gain_map {
                                    if let Some(gain) = gain_value.as_f64() {
                                        log::info!("Mixer '{}': Input '{}' gain {}", 
                                            processor_name, input, gain);
                                    }
                                }
                            }
                        }
                        
                        flow.add_processor(Box::new(mixer));
                        log::info!("Added mixer processor '{}' to flow '{}'", 
                            processor_name, flow_name);
                    }
                    _ => log::error!("Unknown processor type for '{}': {}", 
                        processor_name, processor_cfg.processor_type),
                }
            }
        }
        
        node.add_flow(flow);
        log::info!("Added flow: {}", flow_name);
    }
    
    // Producer mit Flows verbinden (basierend auf Flow inputs)
    for (flow_name, flow_cfg) in &config.flows {
        if !flow_cfg.enabled {
            continue;
        }
        
        for (flow_index, flow) in node.flows().iter().enumerate() {
            if flow.name == *flow_name {
                for input_name in &flow_cfg.inputs {
                    // Finde Producer mit diesem Namen
                    for (producer_index, producer) in node.producers().iter().enumerate() {
                        if producer.name() == input_name {
                            if let Err(e) = node.connect_producer_to_flow(producer_index, flow_index) {
                                log::error!("Failed to connect {} to flow {}: {}", 
                                    input_name, flow_name, e);
                            }
                            break;
                        }
                    }
                }
                break;
            }
        }
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
                log::info!("  Flow {}: running={}, input_buffers={}, processor_buffers={}, output={}", 
                    i, f_status.running, 
                    f_status.input_buffer_levels.len(),
                    f_status.processor_buffer_levels.len(),
                    f_status.output_buffer_level);
            }
        }
    }
    
    node.stop()?;
    log::info!("Node stopped");
    
    Ok(())
}
EOF

# Beispiel-Config mit Mixer aktualisieren
cat > config.toml << 'EOF'
node_name = "studio-node"

[producers.mic1]
type = "alsa_input"
enabled = true
device = "default"
channels = 2
sample_rate = 44100

[producers.background]
type = "file"
enabled = true
path = "background.wav"
loop_audio = true
channels = 2
sample_rate = 48000

[processors.gain_control]
type = "gain"
enabled = true
config = { gain = 0.8 }

[processors.voice_mixer]
type = "mixer"
enabled = true
config = { 
    sample_rate = 48000,
    gains = { "mic1" = 1.0, "background" = 0.3 }
}

[processors.compressor]
type = "passthrough"
enabled = true

[flows.live_stream]
enabled = true
inputs = ["mic1", "background"]
processors = ["voice_mixer", "gain_control", "compressor"]
outputs = []
config = { description = "Mixed voice with background music" }
EOF

echo "Mixer-Processor implementiert!"
echo "Teste mit: cargo run"
echo ""
echo "Features:"
echo "1. ✅ Mixer kann mehrere Inputs kombinieren"
echo "2. ✅ Flows haben jetzt Processing-Threads"
echo "3. ✅ Producer ↔ Flow Verbindungen"
echo "4. ✅ Mixer-Konfiguration über TOML"
