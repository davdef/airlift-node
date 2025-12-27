# 1. Consumer-Trait + FileConsumer implementieren
cat > src/core/consumer.rs << 'EOF'
use std::sync::Arc;
use anyhow::Result;
use crate::core::ringbuffer::AudioRingBuffer;

pub trait Consumer: Send + Sync {
    fn name(&self) -> &str;
    fn start(&mut self) -> Result<()>;
    fn stop(&mut self) -> Result<()>;
    fn status(&self) -> ConsumerStatus;
    fn attach_input_buffer(&mut self, buffer: Arc<AudioRingBuffer>);
}

#[derive(Debug, Clone)]
pub struct ConsumerStatus {
    pub running: bool,
    pub connected: bool,
    pub frames_processed: u64,
    pub bytes_written: u64,
    pub errors: u64,
}

// File Consumer: Schreibt PCM-Daten in WAV-Datei
pub mod file_writer {
    use super::*;
    use std::fs::File;
    use std::io::{Write, BufWriter};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    
    pub struct FileConsumer {
        name: String,
        running: Arc<AtomicBool>,
        input_buffer: Option<Arc<AudioRingBuffer>>,
        output_path: String,
        thread_handle: Option<std::thread::JoinHandle<()>>,
        frames_processed: Arc<AtomicU64>,
        bytes_written: Arc<AtomicU64>,
    }
    
    impl FileConsumer {
        pub fn new(name: &str, output_path: &str) -> Self {
            Self {
                name: name.to_string(),
                running: Arc::new(AtomicBool::new(false)),
                input_buffer: None,
                output_path: output_path.to_string(),
                thread_handle: None,
                frames_processed: Arc::new(AtomicU64::new(0)),
                bytes_written: Arc::new(AtomicU64::new(0)),
            }
        }
        
        fn write_wav_header(writer: &mut BufWriter<File>, sample_rate: u32, channels: u16, bits_per_sample: u16) -> Result<()> {
            // RIFF Header
            writer.write_all(b"RIFF")?;
            writer.write_all(&0u32.to_le_bytes())?; // Placeholder für file size
            writer.write_all(b"WAVE")?;
            
            // fmt chunk
            writer.write_all(b"fmt ")?;
            writer.write_all(&16u32.to_le_bytes())?; // fmt chunk size
            writer.write_all(&1u16.to_le_bytes())?;  // PCM format
            writer.write_all(&channels.to_le_bytes())?;
            writer.write_all(&sample_rate.to_le_bytes())?;
            
            let byte_rate = sample_rate as u32 * channels as u32 * bits_per_sample as u32 / 8;
            writer.write_all(&byte_rate.to_le_bytes())?;
            
            let block_align = channels as u16 * bits_per_sample as u16 / 8;
            writer.write_all(&block_align.to_le_bytes())?;
            writer.write_all(&bits_per_sample.to_le_bytes())?;
            
            // data chunk
            writer.write_all(b"data")?;
            writer.write_all(&0u32.to_le_bytes())?; // Placeholder für data size
            
            Ok(())
        }
        
        fn update_wav_header(file: &mut File, data_size: u32) -> Result<()> {
            // Update RIFF chunk size
            let file_size = data_size + 36; // 36 = Header size ohne RIFF chunk
            file.seek(std::io::SeekFrom::Start(4))?;
            file.write_all(&file_size.to_le_bytes())?;
            
            // Update data chunk size
            file.seek(std::io::SeekFrom::Start(40))?;
            file.write_all(&data_size.to_le_bytes())?;
            
            Ok(())
        }
    }
    
    impl Consumer for FileConsumer {
        fn name(&self) -> &str {
            &self.name
        }
        
        fn start(&mut self) -> Result<()> {
            if self.running.load(Ordering::Relaxed) {
                return Ok(());
            }
            
            log::info!("FileConsumer '{}' starting to write to {}", self.name, self.output_path);
            self.running.store(true, Ordering::SeqCst);
            
            let running = self.running.clone();
            let input_buffer = self.input_buffer.clone();
            let output_path = self.output_path.clone();
            let frames_processed = self.frames_processed.clone();
            let bytes_written = self.bytes_written.clone();
            
            let handle = std::thread::spawn(move || {
                // Erstelle WAV-Datei
                match File::create(&output_path) {
                    Ok(file) => {
                        let mut writer = BufWriter::new(file);
                        
                        // Schreibe Platzhalter-Header
                        if let Err(e) = Self::write_wav_header(&mut writer, 48000, 2, 16) {
                            log::error!("Failed to write WAV header: {}", e);
                            return;
                        }
                        
                        let mut total_samples: u32 = 0;
                        
                        while running.load(Ordering::Relaxed) {
                            if let Some(buffer) = &input_buffer {
                                if let Some(frame) = buffer.pop() {
                                    // Schreibe PCM-Daten
                                    for sample in &frame.samples {
                                        if let Err(e) = writer.write_all(&sample.to_le_bytes()) {
                                            log::error!("Write error: {}", e);
                                            break;
                                        }
                                        bytes_written.fetch_add(2, Ordering::Relaxed); // 2 bytes pro sample
                                    }
                                    
                                    total_samples += frame.samples.len() as u32;
                                    frames_processed.fetch_add(1, Ordering::Relaxed);
                                    
                                    // Alle 10 Frames flushen
                                    if frames_processed.load(Ordering::Relaxed) % 10 == 0 {
                                        if let Err(e) = writer.flush() {
                                            log::error!("Flush error: {}", e);
                                        }
                                    }
                                } else {
                                    std::thread::sleep(std::time::Duration::from_millis(10));
                                }
                            } else {
                                std::thread::sleep(std::time::Duration::from_millis(100));
                            }
                        }
                        
                        // Finalize WAV-Datei
                        if let Ok(mut file) = writer.into_inner() {
                            let data_size = total_samples * 2; // 2 bytes pro sample (16-bit)
                            if let Err(e) = Self::update_wav_header(&mut file, data_size) {
                                log::error!("Failed to update WAV header: {}", e);
                            }
                            if let Err(e) = file.sync_all() {
                                log::error!("Failed to sync file: {}", e);
                            }
                        }
                        
                        log::info!("FileConsumer stopped. Wrote {} frames to {}", 
                            frames_processed.load(Ordering::Relaxed), output_path);
                    }
                    Err(e) => {
                        log::error!("Failed to create file {}: {}", output_path, e);
                    }
                }
            });
            
            self.thread_handle = Some(handle);
            Ok(())
        }
        
        fn stop(&mut self) -> Result<()> {
            log::info!("FileConsumer '{}' stopping...", self.name);
            self.running.store(false, Ordering::SeqCst);
            
            if let Some(handle) = self.thread_handle.take() {
                if let Err(e) = handle.join() {
                    log::error!("Failed to join consumer thread: {:?}", e);
                }
            }
            
            Ok(())
        }
        
        fn status(&self) -> ConsumerStatus {
            ConsumerStatus {
                running: self.running.load(Ordering::Relaxed),
                connected: self.input_buffer.is_some(),
                frames_processed: self.frames_processed.load(Ordering::Relaxed),
                bytes_written: self.bytes_written.load(Ordering::Relaxed),
                errors: 0,
            }
        }
        
        fn attach_input_buffer(&mut self, buffer: Arc<AudioRingBuffer>) {
            self.input_buffer = Some(buffer);
            log::info!("FileConsumer '{}' attached to buffer", self.name);
        }
    }
}
EOF

# 2. Core-Modul Consumer exportieren
cat > src/core/mod.rs << 'EOF'
pub mod device_scanner;
pub mod ringbuffer;
pub mod timestamp;
pub mod processor;
pub mod node;
pub mod consumer;

pub use ringbuffer::*;
pub use timestamp::*;
pub use node::{AirliftNode, Flow};
pub use consumer::{Consumer, ConsumerStatus};

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

# 3. Node für Consumer erweitern
cat > src/core/node.rs << 'EOF'
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use anyhow::Result;
use log::{info, error, warn};

use super::ringbuffer::AudioRingBuffer;
use super::processor::{Processor, ProcessorStatus};
use super::consumer::{Consumer, ConsumerStatus};

pub struct Flow {
    pub name: String,
    pub input_buffers: Vec<Arc<AudioRingBuffer>>,
    pub processor_buffers: Vec<Arc<AudioRingBuffer>>,
    pub output_buffer: Arc<AudioRingBuffer>,
    processors: Vec<Box<dyn Processor>>,
    consumers: Vec<Box<dyn Consumer>>,
    running: Arc<AtomicBool>,
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
            consumers: Vec::new(),
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
    
    pub fn add_consumer(&mut self, consumer: Box<dyn Consumer>) {
        consumer.attach_input_buffer(self.output_buffer.clone());
        self.consumers.push(consumer);
        log::info!("Flow '{}': Added consumer '{}'", self.name, consumer.name());
    }
    
    pub fn start(&mut self) -> Result<()> {
        info!("Flow '{}' starting...", self.name);
        self.running.store(true, Ordering::SeqCst);
        
        // Starte Processing-Thread
        let running = self.running.clone();
        let input_buffers = self.input_buffers.clone();
        let processor_buffers = self.processor_buffers.clone();
        let output_buffer = self.output_buffer.clone();
        let flow_name = self.name.clone();
        
        // Prozessoren für Thread vorbereiten (vereinfacht)
        let mut thread_processors: Vec<Box<dyn Processor>> = Vec::new();
        for processor in &self.processors {
            // Einfache PassThrough als Platzhalter (später echte Cloning-Logik)
            thread_processors.push(Box::new(super::processor::basic::PassThrough::new(processor.name())));
        }
        
        let handle = std::thread::spawn(move || {
            Self::processing_loop(
                running,
                input_buffers,
                processor_buffers,
                output_buffer,
                thread_processors,
                &flow_name,
            );
        });
        
        self.thread_handle = Some(handle);
        
        // Consumer starten
        for consumer in &mut self.consumers {
            if let Err(e) = consumer.start() {
                warn!("Failed to start consumer '{}': {}", consumer.name(), e);
            }
        }
        
        Ok(())
    }
    
    fn processing_loop(
        running: Arc<AtomicBool>,
        input_buffers: Vec<Arc<AudioRingBuffer>>,
        processor_buffers: Vec<Arc<AudioRingBuffer>>,
        output_buffer: Arc<AudioRingBuffer>,
        mut processors: Vec<Box<dyn Processor>>,
        flow_name: &str,
    ) {
        info!("Flow '{}' processing thread started", flow_name);
        
        while running.load(Ordering::Relaxed) {
            if input_buffers.is_empty() {
                std::thread::sleep(std::time::Duration::from_millis(100));
                continue;
            }
            
            // Einfache Pipeline-Verarbeitung
            for (i, processor) in processors.iter_mut().enumerate() {
                let input = if i == 0 {
                    &input_buffers[0] // Erster Prozessor nimmt ersten Input
                } else {
                    &processor_buffers[i - 1]
                };
                
                let output = if i < processor_buffers.len() {
                    &processor_buffers[i]
                } else {
                    &output_buffer
                };
                
                if let Err(e) = processor.process(input, output) {
                    log::error!("Processor '{}' error: {}", processor.name(), e);
                }
            }
            
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        
        info!("Flow '{}' processing thread stopped", flow_name);
    }
    
    pub fn stop(&mut self) -> Result<()> {
        info!("Flow '{}' stopping...", self.name);
        self.running.store(false, Ordering::SeqCst);
        
        // Consumer stoppen
        for consumer in &mut self.consumers {
            if let Err(e) = consumer.stop() {
                warn!("Error stopping consumer '{}': {}", consumer.name(), e);
            }
        }
        
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
        
        let consumer_status: Vec<ConsumerStatus> = 
            self.consumers.iter().map(|c| c.status()).collect();
        
        let input_buffer_levels: Vec<usize> = 
            self.input_buffers.iter().map(|b| b.len()).collect();
        
        let processor_buffer_levels: Vec<usize> = 
            self.processor_buffers.iter().map(|b| b.len()).collect();
        
        FlowStatus {
            running: self.running.load(Ordering::Relaxed),
            processor_status,
            consumer_status,
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
    pub consumer_status: Vec<ConsumerStatus>,
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

# 4. Config für Consumer erweitern
cat > src/config/mod.rs << 'EOF'
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;

#[derive(Debug, Clone, Deserialize, Serialize)]
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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProcessorConfig {
    #[serde(rename = "type")]
    pub processor_type: String,
    pub enabled: bool,
    #[serde(default)]
    pub config: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConsumerConfig {
    #[serde(rename = "type")]
    pub consumer_type: String,
    pub enabled: bool,
    pub path: Option<String>,
    pub url: Option<String>,
    #[serde(default)]
    pub config: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FlowConfig {
    pub enabled: bool,
    pub inputs: Vec<String>,
    pub processors: Vec<String>,
    pub outputs: Vec<String>,
    
    #[serde(default)]
    pub config: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub node_name: String,
    pub producers: HashMap<String, ProducerConfig>,
    pub processors: HashMap<String, ProcessorConfig>,
    pub consumers: HashMap<String, ConsumerConfig>,
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
            consumers: HashMap::new(),
            flows: HashMap::new(),
        }
    }
}
EOF

# 5. Beispiel-Config mit Consumer
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

[processors.gain_control.config]
gain = 0.8

[processors.voice_mixer]
type = "mixer"
enabled = true

[processors.voice_mixer.config]
sample_rate = 48000

[processors.voice_mixer.config.gains]
mic1 = 1.0
background = 0.3

[processors.compressor]
type = "passthrough"
enabled = true

[consumers.recording]
type = "file"
enabled = true
path = "output.wav"

[flows.live_stream]
enabled = true
inputs = ["mic1", "background"]
processors = ["voice_mixer", "gain_control", "compressor"]
outputs = ["recording"]

[flows.live_stream.config]
description = "Mixed voice with background music"
EOF

echo "✅ Consumer implementiert!"
echo "✅ FileConsumer schreibt WAV-Dateien"
echo "✅ Flow kann jetzt Consumer haben"
echo ""
echo "Teste mit: cargo run"
echo "Es sollte eine output.wav Datei erstellen!"
