use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use crate::core::error::{AudioError, AudioResult};
use crate::core::{Event, EventAuditHandler, EventBus, EventPriority, EventType};
#[cfg(feature = "debug-events")]
use crate::core::DebugEventType;

use super::ringbuffer::AudioRingBuffer;
use super::processor::{Processor, ProcessorStatus};
use super::consumer::{Consumer, ConsumerStatus};
use super::lock::lock_mutex;
use super::BufferRegistry;
use crate::core::logging::ComponentLogger;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum PipelineMode {
    Legacy,
    Simplified,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ProcessorBuffering {
    Enabled,
    Disabled,
}

#[derive(Clone, Debug)]
struct ProcessorLink {
    buffer: Option<Arc<AudioRingBuffer>>,
}

#[cfg(feature = "simplified-pipeline")]
const DEFAULT_PIPELINE_MODE: PipelineMode = PipelineMode::Simplified;

#[cfg(not(feature = "simplified-pipeline"))]
const DEFAULT_PIPELINE_MODE: PipelineMode = PipelineMode::Legacy;

pub struct Flow {
    pub name: String,
    pub input_buffers: Vec<Arc<AudioRingBuffer>>,
    pub input_merge_buffer: Arc<AudioRingBuffer>,
    pub processor_buffers: Vec<Arc<AudioRingBuffer>>,
    pub output_buffer: Arc<AudioRingBuffer>,
    processors: Vec<Box<dyn Processor>>,
    consumers: Vec<Box<dyn Consumer>>,
    pipeline_mode: PipelineMode,
    processor_links: Vec<ProcessorLink>,
    scratch_buffers: [Arc<AudioRingBuffer>; 2],
    running: Arc<AtomicBool>,
    thread_handle: Option<std::thread::JoinHandle<()>>,
}

impl Flow {
    pub fn new(name: &str) -> Self {
        let flow = Self {
            name: name.to_string(),
            input_buffers: Vec::new(),
            input_merge_buffer: Arc::new(AudioRingBuffer::new(1000)),
            processor_buffers: Vec::new(),
            output_buffer: Arc::new(AudioRingBuffer::new(1000)),
            processors: Vec::new(),
            consumers: Vec::new(),
            pipeline_mode: DEFAULT_PIPELINE_MODE,
            processor_links: Vec::new(),
            scratch_buffers: [
                Arc::new(AudioRingBuffer::new(1000)),
                Arc::new(AudioRingBuffer::new(1000)),
            ],
            running: Arc::new(AtomicBool::new(false)),
            thread_handle: None,
        };
        
        flow.info(&format!("Flow '{}' created", name));
        flow
    }

    pub fn pipeline_mode(&self) -> PipelineMode {
        self.pipeline_mode
    }

    pub fn use_simplified_pipeline(&mut self) {
        if self.pipeline_mode == PipelineMode::Simplified {
            return;
        }

        self.pipeline_mode = PipelineMode::Simplified;
        self.processor_links = self.processor_buffers
            .iter()
            .cloned()
            .map(|buffer| ProcessorLink { buffer: Some(buffer) })
            .collect();
    }

    pub fn use_legacy_pipeline(&mut self) {
        if self.pipeline_mode == PipelineMode::Legacy {
            return;
        }

        self.pipeline_mode = PipelineMode::Legacy;
        self.rebuild_legacy_buffers();
    }

    fn rebuild_legacy_buffers(&mut self) {
        self.processor_buffers.clear();
        for link in &mut self.processor_links {
            let buffer = link.buffer.clone().unwrap_or_else(|| Arc::new(AudioRingBuffer::new(1000)));
            link.buffer = Some(buffer.clone());
            self.processor_buffers.push(buffer);
        }
    }
    
    #[deprecated(note = "Use add_input_from_registry to connect buffers by registry name.")]
    pub fn add_input_buffer(&mut self, buffer: Arc<AudioRingBuffer>) {
        let buffer_addr = Arc::as_ptr(&buffer);
        let capacity = buffer.stats().capacity;
        
        self.input_buffers.push(buffer);
        
        // Jetzt können wir loggen, da mutable borrow beendet ist
        self.info(&format!(
            "add_input_buffer: buffer addr={:?}, capacity={}",
            buffer_addr, capacity
        ));
    }

    pub fn add_input_from_registry(&mut self, registry: &BufferRegistry, buffer_name: &str) -> AudioResult<()> {
        let buffer = registry.get(buffer_name)
            .ok_or_else(|| AudioError::BufferNotFound { name: buffer_name.to_string() })?;
        self.add_input_buffer(buffer);
        self.info(&format!("Connected input buffer from registry '{}'", buffer_name));
        Ok(())
    }

    pub fn remove_input_from_registry(&mut self, registry: &BufferRegistry, buffer_name: &str) -> AudioResult<()> {
        let buffer = registry.get(buffer_name)
            .ok_or_else(|| AudioError::BufferNotFound { name: buffer_name.to_string() })?;

        let before = self.input_buffers.len();
        self.input_buffers.retain(|candidate| !Arc::ptr_eq(candidate, &buffer));
        if self.input_buffers.len() == before {
            return Err(AudioError::message(format!(
                "buffer '{}' is not connected to flow '{}'",
                buffer_name,
                self.name
            )));
        }

        self.info(&format!("Disconnected input buffer from registry '{}'", buffer_name));
        Ok(())
    }
    
    pub fn add_processor(&mut self, processor: Box<dyn Processor>) {
        self.add_processor_with_buffering(processor, ProcessorBuffering::Enabled);
    }

    pub fn add_processor_unbuffered(&mut self, processor: Box<dyn Processor>) {
        self.add_processor_with_buffering(processor, ProcessorBuffering::Disabled);
    }

    pub fn add_processor_with_buffering(
        &mut self,
        processor: Box<dyn Processor>,
        buffering: ProcessorBuffering,
    ) {
        let processor_name = processor.name().to_string();

        match self.pipeline_mode {
            PipelineMode::Legacy => {
                let buffer = Arc::new(AudioRingBuffer::new(1000));
                self.processor_buffers.push(buffer.clone());
                self.processor_links.push(ProcessorLink { buffer: Some(buffer) });
            }
            PipelineMode::Simplified => {
                let buffer = match buffering {
                    ProcessorBuffering::Enabled => {
                        let buffer = Arc::new(AudioRingBuffer::new(1000));
                        self.processor_buffers.push(buffer.clone());
                        Some(buffer)
                    }
                    ProcessorBuffering::Disabled => None,
                };
                self.processor_links.push(ProcessorLink { buffer });
            }
        }

        self.processors.push(processor);

        // Logging nach mutable borrow
        self.info(&format!("Added processor '{}'", processor_name));
    }
    
    pub fn add_consumer(&mut self, mut consumer: Box<dyn Consumer>) {
        let consumer_name = consumer.name().to_string();
        consumer.attach_input_buffer(self.output_buffer.clone());
        
        self.consumers.push(consumer);
        
        // Logging nach mutable borrow
        self.info(&format!("Added consumer '{}'", consumer_name));
    }
    
    pub fn start(&mut self) -> AudioResult<()> {
        self.info("Starting flow...");
        
        if self.running.load(Ordering::Relaxed) {
            self.warn("Flow already running");
            return Ok(());
        }
        
        self.running.store(true, Ordering::SeqCst);
        
        // Starte Processing-Thread
        let running = self.running.clone();
        let input_buffers = self.input_buffers.clone();
        let input_merge_buffer = self.input_merge_buffer.clone();
        let processor_buffers = self.processor_buffers.clone();
        let output_buffer = self.output_buffer.clone();
        let processor_links = self.processor_links.clone();
        let pipeline_mode = self.pipeline_mode;
        let scratch_buffers = self.scratch_buffers.clone();
        let flow_name = self.name.clone();
        let flow_reader_id = format!("flow:{}:input", self.name);
        
        // Prozessoren für Thread vorbereiten
        let mut thread_processors: Vec<Box<dyn Processor>> = Vec::new();
        for processor in &self.processors {
            thread_processors.push(Box::new(super::processor::basic::PassThrough::new(processor.name())));
        }
        
        let handle = std::thread::spawn(move || {
            match pipeline_mode {
                PipelineMode::Legacy => {
                    Self::processing_loop_legacy(
                        running,
                        input_buffers,
                        input_merge_buffer,
                        processor_buffers,
                        output_buffer,
                        thread_processors,
                        &flow_name,
                        &flow_reader_id,
                    );
                }
                PipelineMode::Simplified => {
                    Self::processing_loop_simplified(
                        running,
                        input_buffers,
                        input_merge_buffer,
                        output_buffer,
                        scratch_buffers,
                        processor_links,
                        thread_processors,
                        &flow_name,
                        &flow_reader_id,
                    );
                }
            }
        });
        
        self.thread_handle = Some(handle);
        
        // Consumer starten - Namen vorher sammeln
        let consumer_names: Vec<String> = self.consumers.iter().map(|c| c.name().to_string()).collect();
        let mut start_errors = Vec::new();
        
        for (i, consumer) in self.consumers.iter_mut().enumerate() {
            let consumer_name = &consumer_names[i];
            if let Err(e) = consumer.start() {
                start_errors.push((consumer_name.clone(), e));
            }
        }
        
        // Jetzt loggen (nach mutable borrow)
        for (consumer_name, error) in &start_errors {
            self.warn(&format!("Failed to start consumer '{}': {}", consumer_name, error));
        }
        
        let successful_starts = consumer_names.len() - start_errors.len();
        if successful_starts > 0 {
            self.info(&format!("{} consumer(s) started successfully", successful_starts));
        }
        
        if start_errors.is_empty() {
            self.info("Flow started successfully");
        } else {
            self.warn(&format!("Flow started with {} error(s)", start_errors.len()));
        }
        
        Ok(())
    }
    
    fn processing_loop_legacy(
        running: Arc<AtomicBool>,
        input_buffers: Vec<Arc<AudioRingBuffer>>,
        input_merge_buffer: Arc<AudioRingBuffer>,
        processor_buffers: Vec<Arc<AudioRingBuffer>>,
        output_buffer: Arc<AudioRingBuffer>,
        mut processors: Vec<Box<dyn Processor>>,
        flow_name: &str,
        flow_reader_id: &str,
    ) {
        // Erstelle einen Logger für den Thread
        let flow_logger = FlowLogger { name: flow_name.to_string() };
        flow_logger.info(&format!("Processing thread started with {} input buffers", 
            input_buffers.len()));
        
        let mut iteration = 0;
        while running.load(Ordering::Relaxed) {
            iteration += 1;
            
            if input_buffers.is_empty() {
                std::thread::sleep(std::time::Duration::from_millis(100));
                continue;
            }
            
            // Sammle Frames von allen Input-Buffern
            let mut frames_collected = 0;
            for buffer in &input_buffers {
                while let Some(frame) = buffer.pop_for_reader(flow_reader_id) {
                    input_merge_buffer.push(frame);
                    frames_collected += 1;
                }
            }
            
            // Log alle 100 Iterationen
            if iteration % 100 == 0 {
                let total_frames: usize = input_buffers.iter().map(|b| b.len()).sum();
                let total_available: usize = input_buffers.iter()
                    .map(|b| b.available_for_reader(flow_reader_id))
                    .sum();
                
                flow_logger.debug(&format!(
                    "Iteration {}: collected={}, total_frames={}, available={}, processors={}",
                    iteration, frames_collected, total_frames, total_available, processors.len()
                ));
            }
            
            // Einfache Pipeline-Verarbeitung
            for (i, processor) in processors.iter_mut().enumerate() {
                let input = if i == 0 {
                    &input_merge_buffer
                } else {
                    &processor_buffers[i - 1]
                };
                
                let output = if i < processor_buffers.len() {
                    &processor_buffers[i]
                } else {
                    &output_buffer
                };
                
                if let Err(e) = processor.process(input, output) {
                    flow_logger.error(&format!("Processor '{}' error: {}", processor.name(), e));
                }
            }
            
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        
        flow_logger.info("Processing thread stopped");
    }

    fn processing_loop_simplified(
        running: Arc<AtomicBool>,
        input_buffers: Vec<Arc<AudioRingBuffer>>,
        input_merge_buffer: Arc<AudioRingBuffer>,
        output_buffer: Arc<AudioRingBuffer>,
        scratch_buffers: [Arc<AudioRingBuffer>; 2],
        processor_links: Vec<ProcessorLink>,
        mut processors: Vec<Box<dyn Processor>>,
        flow_name: &str,
        flow_reader_id: &str,
    ) {
        let flow_logger = FlowLogger { name: flow_name.to_string() };
        flow_logger.info(&format!(
            "Processing thread started (simplified) with {} input buffers",
            input_buffers.len()
        ));

        let mut iteration = 0;
        while running.load(Ordering::Relaxed) {
            iteration += 1;

            if input_buffers.is_empty() {
                std::thread::sleep(std::time::Duration::from_millis(100));
                continue;
            }

            let mut frames_collected = 0;
            for buffer in &input_buffers {
                while let Some(frame) = buffer.pop_for_reader(flow_reader_id) {
                    input_merge_buffer.push(frame);
                    frames_collected += 1;
                }
            }

            if iteration % 100 == 0 {
                let total_frames: usize = input_buffers.iter().map(|b| b.len()).sum();
                let total_available: usize = input_buffers
                    .iter()
                    .map(|b| b.available_for_reader(flow_reader_id))
                    .sum();

                flow_logger.debug(&format!(
                    "Iteration {}: collected={}, total_frames={}, available={}, processors={} (simplified)",
                    iteration,
                    frames_collected,
                    total_frames,
                    total_available,
                    processors.len()
                ));
            }

            let mut current_input = input_merge_buffer.clone();
            let mut scratch_index = 0;

            for (i, processor) in processors.iter_mut().enumerate() {
                let is_last = i + 1 == processors.len();
                let link_buffer = processor_links
                    .get(i)
                    .and_then(|link| link.buffer.clone());

                let output = if is_last {
                    output_buffer.clone()
                } else if let Some(buffer) = link_buffer {
                    buffer
                } else {
                    let buffer = scratch_buffers[scratch_index].clone();
                    scratch_index = (scratch_index + 1) % scratch_buffers.len();
                    buffer
                };

                if let Err(e) = processor.process(&current_input, &output) {
                    flow_logger.error(&format!("Processor '{}' error: {}", processor.name(), e));
                }

                current_input = output;
            }

            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        flow_logger.info("Processing thread stopped (simplified)");
    }
    
    pub fn stop(&mut self) -> AudioResult<()> {
        self.info("Stopping flow...");
        self.running.store(false, Ordering::SeqCst);
        
        // Consumer stoppen - Namen vorher sammeln
        let consumer_names: Vec<String> = self.consumers.iter().map(|c| c.name().to_string()).collect();
        let mut stop_errors = Vec::new();
        
        for (i, consumer) in self.consumers.iter_mut().enumerate() {
            let consumer_name = &consumer_names[i];
            if let Err(e) = consumer.stop() {
                stop_errors.push((consumer_name.clone(), e));
            }
        }
        
        // Jetzt loggen (nach mutable borrow)
        for (consumer_name, error) in &stop_errors {
            self.warn(&format!("Error stopping consumer '{}': {}", consumer_name, error));
        }
        
        let successful_stops = consumer_names.len() - stop_errors.len();
        if successful_stops > 0 {
            self.info(&format!("{} consumer(s) stopped successfully", successful_stops));
        }
        
        if let Some(handle) = self.thread_handle.take() {
            if let Err(e) = handle.join() {
                self.error(&format!("Failed to join flow thread: {:?}", e));
            }
        }
        
        if stop_errors.is_empty() {
            self.info("Flow stopped successfully");
        } else {
            self.warn(&format!("Flow stopped with {} error(s)", stop_errors.len()));
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
        
        let processor_buffer_levels: Vec<usize> = match self.pipeline_mode {
            PipelineMode::Legacy => self.processor_buffers.iter().map(|b| b.len()).collect(),
            PipelineMode::Simplified => self
                .processor_links
                .iter()
                .filter_map(|link| link.buffer.as_ref())
                .map(|buffer| buffer.len())
                .collect(),
        };
        
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

// Helper struct für Thread-Logging
struct FlowLogger {
    name: String,
}

impl crate::core::logging::ComponentLogger for FlowLogger {
    fn log_context(&self) -> crate::core::logging::LogContext {
        crate::core::logging::LogContext::new("Flow", &self.name)
    }
}

// Implementierung des ComponentLogger Traits für Flow
impl crate::core::logging::ComponentLogger for Flow {
    fn log_context(&self) -> crate::core::logging::LogContext {
        crate::core::logging::LogContext::new("Flow", &self.name)
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
    pub flows: Vec<Flow>,
    buffer_registry: Arc<BufferRegistry>,
    event_bus: Arc<Mutex<EventBus>>,
}

impl AirliftNode {
    pub fn new() -> Self {
        // EventBus erstellen und Standard-Handler registrieren
        let mut event_bus = EventBus::new("airlift_node");
        
        // Standard-Handler registrieren
        let audit_handler = Arc::new(EventAuditHandler::new(
            "node_event_audit",
            EventPriority::Info,
        ));
        let _ = event_bus.register_handler(audit_handler);
        
        // EventBus starten
        event_bus.start().expect("Failed to start EventBus");

        let node = Self {
            running: Arc::new(AtomicBool::new(false)),
            start_time: Instant::now(),
            producers: Vec::new(),
            producer_buffers: Vec::new(),
            flows: Vec::new(),
            buffer_registry: Arc::new(BufferRegistry::new()),
            event_bus: Arc::new(Mutex::new(event_bus)),
        };
        
        node.info("AirliftNode created with buffer registry");
        node
    }
    
    pub fn publish_event(&self, event_type: EventType, priority: EventPriority, payload: serde_json::Value) {
        let event = Event::new(
            event_type,
            priority,
            "AirliftNode",
            "main",
            payload,
        ).with_context(self.log_context());
        
        let event_bus = lock_mutex(&self.event_bus, "airlift_node.publish_event");
        if let Err(e) = event_bus.publish(event) {
            self.error(&format!("Failed to publish event: {}", e));
        }
    }

    pub fn buffer_registry(&self) -> Arc<BufferRegistry> {
        self.buffer_registry.clone()
    }
    
    pub fn add_producer(&mut self, producer: Box<dyn super::Producer>) -> AudioResult<()> {
        let producer_name = producer.name().to_string();
        let buffer = Arc::new(AudioRingBuffer::new(1000));
        
        let mut producer = producer;
        producer.attach_ring_buffer(buffer.clone());
        
        // Buffer in Registry registrieren
        let buffer_name = format!("producer:{}", producer_name);
        if let Err(e) = self.buffer_registry.register(&buffer_name, buffer.clone()) {
            self.error(&format!("Failed to register buffer '{}': {}", buffer_name, e));
            return Err(AudioError::with_context(
                format!("register buffer '{}'", buffer_name),
                e,
            ));
        }
        
        self.producer_buffers.push(buffer);
        self.producers.push(producer);
        
        self.publish_event(EventType::ConfigChanged, EventPriority::Info,
            serde_json::json!({
                "action": "producer_added",
                "producer_name": producer_name,
                "buffer_name": buffer_name,
                "timestamp": crate::core::timestamp::utc_ns_now(),
            }));

        self.info(&format!("Added producer '{}' (buffer: '{}')", producer_name, buffer_name));
        Ok(())
    }
    
    pub fn add_flow(&mut self, flow: Flow) {
        let flow_name = flow.name.clone();
        self.flows.push(flow);
        
        // Logging nach mutable borrow
        self.info(&format!("Added flow: '{}'", flow_name));
    }
    
    pub fn connect_flow_input(&mut self, flow_index: usize, buffer_name: &str) -> AudioResult<()> {
        if flow_index >= self.flows.len() {
            return Err(AudioError::InvalidFlowIndex {
                index: flow_index,
                max: self.flows.len().saturating_sub(1),
            });
        }

        self.flows[flow_index].add_input_from_registry(&self.buffer_registry, buffer_name)?;

        self.info(&format!(
            "Connected registry buffer '{}' to flow {}",
            buffer_name, flow_index
        ));

        Ok(())
    }

    pub fn disconnect_flow_input(&mut self, flow_index: usize, buffer_name: &str) -> AudioResult<()> {
        if flow_index >= self.flows.len() {
            return Err(AudioError::InvalidFlowIndex {
                index: flow_index,
                max: self.flows.len().saturating_sub(1),
            });
        }

        self.flows[flow_index].remove_input_from_registry(&self.buffer_registry, buffer_name)?;

        self.info(&format!(
            "Disconnected registry buffer '{}' from flow {}",
            buffer_name, flow_index
        ));

        Ok(())
    }

    /// Deprecated: use registry-based connection via connect_flow_input instead.
    /// Transition plan: keep deprecated during the current release line, then remove in a major version bump.
    #[deprecated(note = "Use connect_flow_input instead.")]
    pub fn connect_registered_buffer_to_flow(&mut self, buffer_name: &str, flow_index: usize) -> AudioResult<()> {
        self.connect_flow_input(flow_index, buffer_name)
    }

    /// Deprecated: use registry-based connection instead.
    /// Transition plan: keep deprecated during the current release line, then remove in a major version bump.
    #[deprecated(note = "Use registry-based connection via connect_flow_input instead.")]
    pub fn connect_producer_to_flow(&mut self, producer_index: usize, flow_index: usize) -> AudioResult<()> {
        self.warn("connect_producer_to_flow is deprecated; use connect_flow_input instead.");

        let producer_name = self.producers.get(producer_index)
            .map(|producer| producer.name().to_string())
            .ok_or_else(|| AudioError::InvalidProducerIndex {
                index: producer_index,
                max: self.producers.len().saturating_sub(1),
            })?;

        let buffer_name = format!("producer:{}", producer_name);
        self.connect_flow_input(flow_index, &buffer_name)
    }

    /// Deprecated: use connect_flow_input instead.
    #[deprecated(note = "Use connect_flow_input instead.")]
    pub fn connect_registry_to_flow(&mut self, flow_index: usize, buffer_name: &str) -> AudioResult<()> {
        self.connect_flow_input(flow_index, buffer_name)
    }
    
    /// Erstelle und füge einen Mixer mit Buffer-Registry hinzu
    pub fn create_and_add_mixer(&mut self, flow_index: usize, name: &str, config: crate::processors::MixerConfig) -> AudioResult<()> {
        if flow_index < self.flows.len() {
            let mut mixer = crate::processors::Mixer::from_config(name, config);
            mixer.set_buffer_registry(self.buffer_registry());
            
            // Versuche automatisch zu verbinden
            if let Err(e) = mixer.connect_from_registry() {
                self.warn(&format!("Mixer '{}' auto-connect failed: {}", name, e));
                // Nicht fatal, Mixer kann später verbunden werden
            }
            
            self.flows[flow_index].add_processor(Box::new(mixer));
            self.info(&format!("Added mixer '{}' to flow {}", name, flow_index));
            Ok(())
        } else {
            Err(AudioError::InvalidFlowIndex {
                index: flow_index,
                max: self.flows.len().saturating_sub(1),
            })
        }
    }
    
    /// Füge einen beliebigen Processor hinzu
    pub fn add_processor_to_flow(&mut self, flow_index: usize, processor: Box<dyn Processor>) -> AudioResult<()> {
        if flow_index < self.flows.len() {
            self.flows[flow_index].add_processor(processor);
            Ok(())
        } else {
            Err(AudioError::InvalidFlowIndex {
                index: flow_index,
                max: self.flows.len().saturating_sub(1),
            })
        }
    }
    
    pub fn start(&mut self) -> AudioResult<()> {
        self.info("Node starting...");

        #[cfg(feature = "debug-events")]
        self.publish_event(EventType::Debug(DebugEventType::NodeStarted), EventPriority::Info,
            serde_json::json!({
                "timestamp": crate::core::timestamp::utc_ns_now(),
                "producers": self.producers.len(),
                "flows": self.flows.len(),
            }));
        
        if self.running.load(Ordering::Relaxed) {
            self.warn("Node already running");
            return Ok(());
        }
        
        self.running.store(true, Ordering::SeqCst);
        
        // Producer starten - Namen vorher sammeln
        let producer_names: Vec<String> = self.producers.iter().map(|p| p.name().to_string()).collect();
        let mut start_errors = Vec::new();
        
        for (i, producer) in self.producers.iter_mut().enumerate() {
            let producer_name = &producer_names[i];
            if let Err(e) = producer.start() {
                start_errors.push((producer_name.clone(), e));
            }
        }
        
        // Jetzt loggen (nach mutable borrow)
        for (producer_name, error) in &start_errors {
            self.error(&format!("Failed to start producer '{}': {}", producer_name, error));
        }
        
        let successful_starts = producer_names.len() - start_errors.len();
        if successful_starts > 0 {
            self.info(&format!("{} producer(s) started successfully", successful_starts));
        }
        
        // Flows starten - Namen vorher sammeln
        let flow_names: Vec<String> = self.flows.iter().map(|f| f.name.clone()).collect();
        let mut flow_start_errors = Vec::new();
        
        for (i, flow) in self.flows.iter_mut().enumerate() {
            let flow_name = &flow_names[i];
            if let Err(e) = flow.start() {
                flow_start_errors.push((flow_name.clone(), e));
            }
        }
        
        // Loggen
        for (flow_name, error) in &flow_start_errors {
            self.warn(&format!("Failed to start flow '{}': {}", flow_name, error));
        }
        
        let successful_flows = flow_names.len() - flow_start_errors.len();
        if successful_flows > 0 {
            self.info(&format!("{} flow(s) started successfully", successful_flows));
        }
        
        if start_errors.is_empty() && flow_start_errors.is_empty() {
            self.info("Node started successfully");
        } else {
            let total_errors = start_errors.len() + flow_start_errors.len();
            self.warn(&format!("Node started with {} error(s)", total_errors));
        }
        
        Ok(())
    }
    
    pub fn stop(&mut self) -> AudioResult<()> {
        self.info("Node stopping...");

        #[cfg(feature = "debug-events")]
        self.publish_event(EventType::Debug(DebugEventType::NodeStopped), EventPriority::Info,
            serde_json::json!({
                "timestamp": crate::core::timestamp::utc_ns_now(),
                "uptime_seconds": self.start_time.elapsed().as_secs(),
            }));

        self.running.store(false, Ordering::SeqCst);
        
        // Flows stoppen - Namen vorher sammeln
        let flow_names: Vec<String> = self.flows.iter().map(|f| f.name.clone()).collect();
        let mut flow_stop_errors = Vec::new();
        
        for (i, flow) in self.flows.iter_mut().enumerate() {
            let flow_name = &flow_names[i];
            if let Err(e) = flow.stop() {
                flow_stop_errors.push((flow_name.clone(), e));
            }
        }
        
        // Loggen
        for (flow_name, error) in &flow_stop_errors {
            self.warn(&format!("Error stopping flow '{}': {}", flow_name, error));
        }
        
        let successful_flows = flow_names.len() - flow_stop_errors.len();
        if successful_flows > 0 {
            self.info(&format!("{} flow(s) stopped successfully", successful_flows));
        }
        
        // Producer stoppen - Namen vorher sammeln
        let producer_names: Vec<String> = self.producers.iter().map(|p| p.name().to_string()).collect();
        let mut producer_stop_errors = Vec::new();
        
        for (i, producer) in self.producers.iter_mut().enumerate() {
            let producer_name = &producer_names[i];
            if let Err(e) = producer.stop() {
                producer_stop_errors.push((producer_name.clone(), e));
            }
        }
        
        // Loggen
        for (producer_name, error) in &producer_stop_errors {
            self.warn(&format!("Error stopping producer '{}': {}", producer_name, error));
        }
        
        let successful_producers = producer_names.len() - producer_stop_errors.len();
        if successful_producers > 0 {
            self.info(&format!("{} producer(s) stopped successfully", successful_producers));
        }
        
        let event_bus_stop_error = {
            let mut event_bus = lock_mutex(&self.event_bus, "airlift_node.stop_event_bus");
            match event_bus.stop() {
                Ok(()) => {
                    self.info("EventBus stopped successfully");
                    false
                }
                Err(e) => {
                    self.warn(&format!("Error stopping EventBus: {}", e));
                    true
                }
            }
        };

        if flow_stop_errors.is_empty() && producer_stop_errors.is_empty() && !event_bus_stop_error {
            self.info("Node stopped successfully");
        } else {
            let total_errors = flow_stop_errors.len()
                + producer_stop_errors.len()
                + usize::from(event_bus_stop_error);
            self.warn(&format!("Node stopped with {} error(s)", total_errors));
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

// Implementierung des ComponentLogger Traits für AirliftNode
impl crate::core::logging::ComponentLogger for AirliftNode {
    fn log_context(&self) -> crate::core::logging::LogContext {
        crate::core::logging::LogContext::new("Node", "main")
    }
}

impl Drop for AirliftNode {
    fn drop(&mut self) {
        let mut event_bus = lock_mutex(&self.event_bus, "airlift_node.drop_event_bus");
        if let Err(e) = event_bus.stop() {
            self.warn(&format!("Error stopping EventBus during drop: {}", e));
        }
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

// Unit Tests
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::processor::basic::PassThrough;
    use crate::core::logging::ComponentLogger;

    #[test]
    fn test_flow_creation() {
        let flow = Flow::new("test_flow");
        assert_eq!(flow.name, "test_flow");
        assert!(flow.input_buffers.is_empty());
        assert!(flow.processors.is_empty());
        assert!(flow.consumers.is_empty());
        assert!(!flow.running.load(Ordering::Relaxed));
    }

    #[test]
    fn test_flow_add_components() {
        let mut flow = Flow::new("test_flow");
        
        // Add processor
        let processor = Box::new(PassThrough::new("test_processor"));
        flow.add_processor(processor);
        assert_eq!(flow.processors.len(), 1);
    }

    #[test]
    fn test_flow_simplified_pipeline_buffering() {
        let mut flow = Flow::new("simplified_flow");
        flow.use_simplified_pipeline();

        let processor = Box::new(PassThrough::new("unbuffered"));
        flow.add_processor_unbuffered(processor);

        let processor = Box::new(PassThrough::new("buffered"));
        flow.add_processor(processor);

        assert_eq!(flow.pipeline_mode(), PipelineMode::Simplified);
        assert_eq!(flow.processors.len(), 2);
        assert_eq!(flow.processor_links.len(), 2);
        assert_eq!(flow.processor_buffers.len(), 1);
    }

    #[test]
    fn test_node_connect_registered_buffer_to_flow() {
        let mut node = AirliftNode::new();
        let flow = Flow::new("test_flow");
        node.add_flow(flow);

        let buffer = Arc::new(AudioRingBuffer::new(100));
        node.buffer_registry()
            .register("test:buffer", buffer)
            .expect("failed to register buffer");

        node.connect_flow_input(0, "test:buffer")
            .expect("failed to connect buffer");

        assert_eq!(node.flows[0].input_buffers.len(), 1);
        
        // Add input buffer
        let registry = BufferRegistry::new();
        let buffer = Arc::new(AudioRingBuffer::new(100));
        registry.register("producer:test", buffer).unwrap();
        flow.add_input_from_registry(&registry, "producer:test").unwrap();
        assert_eq!(flow.input_buffers.len(), 1);
    }

    #[test]
    fn test_node_creation() {
        let node = AirliftNode::new();
        assert!(!node.is_running());
        assert!(node.producers.is_empty());
        assert!(node.flows.is_empty());
    }

    #[test]
    fn test_node_add_flow() {
        let mut node = AirliftNode::new();
        let flow = Flow::new("test_flow");
        
        node.add_flow(flow);
        assert_eq!(node.flows.len(), 1);
        assert_eq!(node.flows[0].name, "test_flow");
    }

    #[test]
    fn test_flow_logging() {
        let flow = Flow::new("logging_test");
        
        // Test dass Logging-Methoden verfügbar sind
        flow.debug("Test debug message");
        flow.info("Test info message");
        flow.warn("Test warning message");
        flow.error("Test error message");
        
        // Test buffer tracing
        flow.trace_buffer(&flow.output_buffer);
    }

    #[test]
    fn test_node_logging() {
        let node = AirliftNode::new();
        
        // Test dass Logging-Methoden verfügbar sind
        node.debug("Test debug message");
        node.info("Test info message");
        node.warn("Test warning message");
        node.error("Test error message");
    }
}
