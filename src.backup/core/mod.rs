// src/core/mod.rs - Korrigiert

use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use anyhow::Result;
use log::{info, error, warn};

// ============================================================================
// SHARED DATA STRUCTURES
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioFormat {
    pub sample_rate: u32,
    pub channels: u8,
    pub sample_type: SampleType,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SampleType {
    F32,
    I16,
}

#[derive(Debug, Clone)]
pub struct EncodedFrame {
    pub payload: Vec<u8>,
    pub codec_info: CodecInfo,
}

#[derive(Debug, Clone)]
pub struct CodecInfo {
    pub kind: CodecKind,
    pub sample_rate: u32,
    pub channels: u8,
    pub container: ContainerKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CodecKind {
    Pcm,
    OpusOgg,
    OpusWebRtc,
    Mp3,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ContainerKind {
    Raw,
    Ogg,
    Mpeg,
    Rtp,
}

// ============================================================================
// RINGBUFFER IMPLEMENTATION
// ============================================================================

pub struct PcmRingBuffer {
    slots: usize,
    buffer: Vec<f32>,
    write_pos: usize,
    read_positions: HashMap<String, usize>,
}

impl PcmRingBuffer {
    pub fn new(slots: usize, prealloc_samples: usize) -> Self {
        Self {
            slots,
            buffer: vec![0.0; prealloc_samples],
            write_pos: 0,
            read_positions: HashMap::new(),
        }
    }
    
    pub fn write(&mut self, samples: &[f32]) -> Result<usize> {
        let available = self.slots - (self.write_pos % self.slots);
        let to_write = samples.len().min(available);
        
        if to_write > 0 {
            let start = self.write_pos % self.buffer.len();
            let end = start + to_write;
            
            if end <= self.buffer.len() {
                self.buffer[start..end].copy_from_slice(&samples[..to_write]);
            } else {
                let first_part = self.buffer.len() - start;
                self.buffer[start..].copy_from_slice(&samples[..first_part]);
                self.buffer[..to_write - first_part].copy_from_slice(&samples[first_part..to_write]);
            }
            
            self.write_pos += to_write;
        }
        
        Ok(to_write)
    }
    
    pub fn read_new_samples(&mut self, consumer_id: &str) -> Result<Vec<f32>> {
        let read_pos = self.read_positions.entry(consumer_id.to_string())
            .or_insert(self.write_pos);
        
        if *read_pos >= self.write_pos {
            return Ok(Vec::new());
        }
        
        let available = self.write_pos - *read_pos;
        let to_read = available.min(self.buffer.len() / 2);
        
        let start = *read_pos % self.buffer.len();
        let end = (start + to_read) % self.buffer.len();
        
        let samples = if end > start {
            self.buffer[start..end].to_vec()
        } else {
            let mut result = Vec::with_capacity(to_read);
            result.extend_from_slice(&self.buffer[start..]);
            result.extend_from_slice(&self.buffer[..end]);
            result
        };
        
        *read_pos += to_read;
        Ok(samples)
    }
}

pub struct EncodedRingBuffer {
    frames: Vec<EncodedFrame>,
    capacity: usize,
    write_pos: usize,
    read_pos: usize,
}

impl EncodedRingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            frames: Vec::with_capacity(capacity),
            capacity,
            write_pos: 0,
            read_pos: 0,
        }
    }
    
    pub fn write(&mut self, frame: EncodedFrame) -> Result<()> {
        if self.frames.len() < self.capacity {
            self.frames.push(frame);
        } else {
            self.frames[self.write_pos % self.capacity] = frame;
        }
        self.write_pos += 1;
        Ok(())
    }
    
    pub fn read(&mut self) -> Option<EncodedFrame> {
        if self.read_pos >= self.write_pos {
            return None;
        }
        
        let idx = self.read_pos % self.capacity;
        self.read_pos += 1;
        
        if idx < self.frames.len() {
            Some(self.frames[idx].clone())
        } else {
            None
        }
    }
}

// ============================================================================
// CORE TRAITS
// ============================================================================

pub trait Producer: Send + Sync {
    fn start(&mut self) -> Result<()>;
    fn stop(&mut self) -> Result<()>;
    fn format(&self) -> AudioFormat;
    fn attach_buffer(&mut self, buffer: Arc<PcmRingBuffer>) -> Result<()>;
    fn status(&self) -> ProducerStatus;
}

pub type SharedPcmBuffer = Arc<Mutex<PcmRingBuffer>>;

pub trait Processor: Send + Sync {
    fn name(&self) -> &str;
    fn attach(&mut self, buffer: SharedPcmBuffer) -> Result<()>;
    fn process(&mut self) -> Result<()>;
    fn detach(&mut self) -> Result<()>;
}

pub trait Encoder: Send + Sync {
    fn name(&self) -> &str;
    fn attach_input(&mut self, buffer: Arc<PcmRingBuffer>) -> Result<()>;
    fn output_buffer(&self) -> Arc<EncodedRingBuffer>;
    fn process(&mut self) -> Result<()>;
}

pub trait Consumer: Send + Sync {
    fn name(&self) -> &str;
    fn start(&mut self, buffer: Arc<EncodedRingBuffer>) -> Result<()>;
    fn stop(&mut self) -> Result<()>;
    fn status(&self) -> ConsumerStatus;
}

pub trait Service: Send + Sync {
    fn name(&self) -> &str;
    fn start(&mut self) -> Result<()>;
    fn stop(&mut self) -> Result<()>;
    fn health(&self) -> ServiceHealth;
}

// ============================================================================
// STATUS STRUCTURES
// ============================================================================

#[derive(Debug, Clone)]
pub struct ProducerStatus {
    pub running: bool,
    pub connected: bool,
    pub samples_written: u64,
    pub errors: u64,
}

#[derive(Debug, Clone)]
pub struct ConsumerStatus {
    pub running: bool,
    pub connected: bool,
    pub frames_written: u64,
    pub errors: u64,
}

#[derive(Debug, Clone)]
pub struct ServiceHealth {
    pub healthy: bool,
    pub uptime: Duration,
    pub metrics: HashMap<String, f64>,
}

// ============================================================================
// NODE BUILDER
// ============================================================================

pub struct NodeBuilder {
    producers: HashMap<String, Box<dyn Producer>>,
    processors: HashMap<String, Arc<Mutex<dyn Processor>>>,
    encoders: HashMap<String, Arc<Mutex<dyn Encoder>>>,
    consumers: HashMap<String, Box<dyn Consumer>>,
    services: HashMap<String, Box<dyn Service>>,
    
    pcm_buffers: HashMap<String, Arc<PcmRingBuffer>>,
    encoded_buffers: HashMap<String, Arc<EncodedRingBuffer>>,
}

impl NodeBuilder {
    pub fn new() -> Self {
        Self {
            producers: HashMap::new(),
            processors: HashMap::new(),
            encoders: HashMap::new(),
            consumers: HashMap::new(),
            services: HashMap::new(),
            pcm_buffers: HashMap::new(),
            encoded_buffers: HashMap::new(),
        }
    }
    
    pub fn add_producer(&mut self, name: String, mut producer: Box<dyn Producer>) -> Result<()> {
        let buffer = Arc::new(PcmRingBuffer::new(5000, 9600));
        producer.attach_buffer(buffer.clone())?;
        
        self.pcm_buffers.insert(name.clone(), buffer);
        self.producers.insert(name, producer);
        Ok(())
    }
    
pub fn add_processor(
    &mut self, 
    name: String, 
    processor: Box<dyn Processor>,
    source: &str
) -> Result<()> {
    let buffer = self.pcm_buffers.get(source)
        .ok_or_else(|| anyhow::anyhow!("Unknown source: {}", source))?;
    
    // Erstelle Mutex vor dem Arc
    let mut processor = processor;
    processor.attach(buffer.clone())?;
    
    // In Arc<Mutex> wrappen - KEIN Box mehr!
    self.processors.insert(name, Arc::new(Mutex::new(processor)));
    Ok(())
}
    
pub fn add_encoder(
    &mut self,
    name: String,
    encoder: Box<dyn Encoder>,
    source: &str
) -> Result<()> {
    let buffer = self.pcm_buffers.get(source)
        .ok_or_else(|| anyhow::anyhow!("Unknown source: {}", source))?;
    
    let mut encoder = encoder;
    encoder.attach_input(buffer.clone())?;
    
    let encoded_buffer = encoder.output_buffer();
    self.encoded_buffers.insert(name.clone(), encoded_buffer);
    
    self.encoders.insert(name, Arc::new(Mutex::new(encoder)));
    Ok(())
}
    
    pub fn add_consumer(
        &mut self,
        name: String,
        mut consumer: Box<dyn Consumer>,
        source: &str
    ) -> Result<()> {
        let buffer = self.encoded_buffers.get(source)
            .ok_or_else(|| anyhow::anyhow!("Unknown encoded source: {}", source))?;
        
        consumer.start(buffer.clone())?;
        self.consumers.insert(name, consumer);
        Ok(())
    }
    
    pub fn add_service(&mut self, name: String, service: Box<dyn Service>) -> Result<()> {
        self.services.insert(name, service);
        Ok(())
    }
    
    pub fn build(self) -> Result<AirliftNode> {
        Ok(AirliftNode {
            producers: self.producers,
            processors: self.processors,
            encoders: self.encoders,
            consumers: self.consumers,
            services: self.services,
            pcm_buffers: self.pcm_buffers,
            encoded_buffers: self.encoded_buffers,
            running: Arc::new(AtomicBool::new(false)),
            start_time: Instant::now(),
        })
    }
}

// ============================================================================
// AIRLIFT NODE
// ============================================================================

pub struct AirliftNode {
    producers: HashMap<String, Box<dyn Producer>>,
    processors: HashMap<String, Arc<Mutex<dyn Processor>>>,
    encoders: HashMap<String, Arc<Mutex<dyn Encoder>>>,
    consumers: HashMap<String, Box<dyn Consumer>>,
    services: HashMap<String, Box<dyn Service>>,
    
    pcm_buffers: HashMap<String, Arc<PcmRingBuffer>>,
    encoded_buffers: HashMap<String, Arc<EncodedRingBuffer>>,
    
    running: Arc<AtomicBool>,
    start_time: Instant,
}

impl AirliftNode {
    pub fn start(&mut self) -> Result<()> {
        info!("Starting Airlift node");
        self.running.store(true, Ordering::SeqCst);
        
        for (name, service) in &mut self.services {
            info!("Starting service: {}", name);
            if let Err(e) = service.start() {
                error!("Failed to start service {}: {}", name, e);
            }
        }
        
        for (name, producer) in &mut self.producers {
            info!("Starting producer: {}", name);
            if let Err(e) = producer.start() {
                error!("Failed to start producer {}: {}", name, e);
            }
        }
        
        self.start_processing_threads()?;
        
        info!("Node started successfully");
        Ok(())
    }
    
    fn start_processing_threads(&self) -> Result<()> {
        let running = self.running.clone();
        
        for (name, processor) in &self.processors {
            let running = running.clone();
            let processor = Arc::clone(processor);
            let name = name.clone();
            
            std::thread::spawn(move || {
                while running.load(Ordering::Relaxed) {
                    if let Ok(mut proc) = processor.lock() {
                        if let Err(e) = proc.process() {
                            error!("Processor {} error: {}", name, e);
                        }
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
            });
        }
        
        for (name, encoder) in &self.encoders {
            let running = running.clone();
            let encoder = Arc::clone(encoder);
            let name = name.clone();
            
            std::thread::spawn(move || {
                while running.load(Ordering::Relaxed) {
                    if let Ok(mut enc) = encoder.lock() {
                        if let Err(e) = enc.process() {
                            error!("Encoder {} error: {}", name, e);
                        }
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            });
        }
        
        Ok(())
    }
    
    pub fn stop(&mut self) -> Result<()> {
        info!("Stopping Airlift node");
        self.running.store(false, Ordering::SeqCst);
        
        std::thread::sleep(Duration::from_millis(100));
        
        for (name, consumer) in &mut self.consumers {
            info!("Stopping consumer: {}", name);
            if let Err(e) = consumer.stop() {
                warn!("Error stopping consumer {}: {}", name, e);
            }
        }
        
        for (name, producer) in &mut self.producers {
            info!("Stopping producer: {}", name);
            if let Err(e) = producer.stop() {
                warn!("Error stopping producer {}: {}", name, e);
            }
        }
        
        for (name, service) in &mut self.services {
            info!("Stopping service: {}", name);
            if let Err(e) = service.stop() {
                warn!("Error stopping service {}: {}", name, e);
            }
        }
        
        Ok(())
    }
    
    pub fn health_check(&self) -> Result<()> {
        let uptime = self.start_time.elapsed();
        
        if uptime < Duration::from_secs(5) {
            return Ok(());
        }
        
        for (name, producer) in &self.producers {
            let status = producer.status();
            if !status.running && uptime > Duration::from_secs(10) {
                return Err(anyhow::anyhow!("Producer {} not running", name));
            }
        }
        
        for (name, consumer) in &self.consumers {
            let status = consumer.status();
            if !status.running && uptime > Duration::from_secs(10) {
                return Err(anyhow::anyhow!("Consumer {} not running", name));
            }
        }
        
        Ok(())
    }
    
    pub fn status_report(&self) -> NodeStatus {
        NodeStatus {
            uptime: self.start_time.elapsed(),
            producers: self.producers.len(),
            processors: self.processors.len(),
            encoders: self.encoders.len(),
            consumers: self.consumers.len(),
            services: self.services.len(),
            running: self.running.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug)]
pub struct NodeStatus {
    pub uptime: Duration,
    pub producers: usize,
    pub processors: usize,
    pub encoders: usize,
    pub consumers: usize,
    pub services: usize,
    pub running: bool,
}
