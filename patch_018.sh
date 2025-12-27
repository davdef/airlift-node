# 1. Processors Modul-Datei erstellen (falls nicht existiert)
cat > src/processors/mod.rs << 'EOF'
// Re-export für alle Processor-Implementierungen
pub mod mixer;
pub use mixer::Mixer;

// Eventuell später: andere Processor-Typen
// pub mod compressor;
// pub mod limiter;
// pub mod equalizer;
EOF

# 2. Core processor.rs den Mixer importieren lassen (korrekter Pfad)
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

// Basis-Processors (können hier bleiben oder in processors/ verschoben werden)
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
EOF

# 3. Node.rs Fix für Move-Error
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
    processors: Vec<Box<dyn Processor>>,
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
        
        // Starte Processing-Thread mit Clones
        let running = self.running.clone();
        let input_buffers = self.input_buffers.clone();
        let processor_buffers = self.processor_buffers.clone();
        let output_buffer = self.output_buffer.clone();
        let flow_name = self.name.clone();
        
        // Prozessoren müssen separat behandelt werden
        let processors_clone: Vec<Box<dyn Processor>> = self.processors.iter_mut()
            .map(|p| {
                // Einfache Box-Klon-Methode (nicht ideal, aber für Demo)
                Box::new(crate::core::processor::basic::PassThrough::new(p.name())) as Box<dyn Processor>
            })
            .collect();
        
        let handle = std::thread::spawn(move || {
            Self::processing_loop(
                running,
                input_buffers,
                processor_buffers,
                output_buffer,
                processors_clone,
                &flow_name,
            );
        });
        
        self.thread_handle = Some(handle);
        
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
                    &processor_buffers[i - 1] // Spätere Prozessoren nehmen Ausgabe des vorherigen
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

# 4. Main für korrekte Processor-Imports anpassen
# Wir müssen Mixer aus processors importieren, nicht aus core
sed -i 's/core::processor::mixer::Mixer/processors::mixer::Mixer/g' src/main.rs
sed -i 's/processors::mixer::Mixer/crate::processors::mixer::Mixer/g' src/main.rs 2>/dev/null || true

# Build testen
echo "Jetzt bauen..."
cargo build
