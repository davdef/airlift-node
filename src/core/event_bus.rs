// src/core/event_bus.rs
use std::sync::{Arc, RwLock};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use anyhow::Result;
use crossbeam_channel::{unbounded, Sender, Receiver, TryRecvError};
use super::events::{Event, EventPriority};
use super::logging::{ComponentLogger, LogContext};

/// Event-Handler Trait
pub trait EventHandler: Send + Sync {
    fn handle_event(&self, event: &Event) -> Result<()>;
    fn name(&self) -> &str;
    fn priority_filter(&self) -> Option<EventPriority> {
        None
    }
    fn event_type_filter(&self) -> Option<Vec<super::events::EventType>> {
        None
    }
}

/// Event-Bus für verteilte Event-Verarbeitung
pub struct EventBus {
    name: String,
    event_tx: Sender<Event>,
    event_rx: Receiver<Event>,
    handlers: Arc<RwLock<Vec<Arc<dyn EventHandler>>>>,
    running: Arc<AtomicBool>,
    thread_handle: Option<std::thread::JoinHandle<()>>,
    event_count: std::sync::atomic::AtomicU64,
}

impl EventBus {
    pub fn new(name: &str) -> Self {
        let (tx, rx) = unbounded();
        
        let bus = Self {
            name: name.to_string(),
            event_tx: tx,
            event_rx: rx,
            handlers: Arc::new(RwLock::new(Vec::new())),
            running: Arc::new(AtomicBool::new(false)),
            thread_handle: None,
            event_count: std::sync::atomic::AtomicU64::new(0),
        };
        
        bus.info(&format!("EventBus '{}' created", name));
        bus
    }
    
    /// Startet den Event-Bus Processing Thread
    pub fn start(&mut self) -> Result<()> {
        if self.running.load(Ordering::Relaxed) {
            return Ok(());
        }
        
        self.info("Starting EventBus...");
        self.running.store(true, Ordering::SeqCst);
        
        let event_rx = self.event_rx.clone();
        let handlers = self.handlers.clone();
        let running = self.running.clone();
        let name = self.name.clone();
        let event_count = self.event_count.clone();
        
        let handle = std::thread::spawn(move || {
            EventBus::processing_loop(event_rx, handlers, running, name, event_count);
        });
        
        self.thread_handle = Some(handle);
        self.info("EventBus started successfully");
        
        Ok(())
    }
    
    /// Stoppt den Event-Bus
    pub fn stop(&mut self) -> Result<()> {
        self.info("Stopping EventBus...");
        self.running.store(false, Ordering::SeqCst);
        
        // Sende Shutdown-Event
        self.publish(Event::new(
            super::events::EventType::NodeStopped,
            super::events::EventPriority::Info,
            "EventBus",
            &self.name,
            serde_json::json!({
                "message": "EventBus shutting down",
                "timestamp": crate::core::timestamp::utc_ns_now(),
            }),
        ))?;
        
        if let Some(handle) = self.thread_handle.take() {
            if let Err(e) = handle.join() {
                self.error(&format!("Failed to join EventBus thread: {:?}", e));
            }
        }
        
        self.info("EventBus stopped");
        Ok(())
    }
    
    /// Event an den Bus senden
    pub fn publish(&self, event: Event) -> Result<()> {
        self.event_count.fetch_add(1, Ordering::Relaxed);
        
        // Log high-priority events
        match event.priority {
            super::events::EventPriority::Error | super::events::EventPriority::Critical => {
                self.error(&event.format_message());
            }
            super::events::EventPriority::Warning => {
                self.warn(&event.format_message());
            }
            super::events::EventPriority::Info => {
                self.info(&event.format_message());
            }
            super::events::EventPriority::Debug => {
                self.debug(&event.format_message());
            }
        }
        
        if let Err(e) = self.event_tx.send(event) {
            self.error(&format!("Failed to publish event: {}", e));
            anyhow::bail!("Failed to publish event: {}", e);
        }
        
        Ok(())
    }
    
    /// Event-Handler registrieren
    pub fn register_handler(&self, handler: Arc<dyn EventHandler>) -> Result<()> {
        let mut handlers = self.handlers.write()
            .map_err(|e| anyhow::anyhow!("Failed to acquire write lock: {}", e))?;
        
        handlers.push(handler.clone());
        
        self.info(&format!(
            "Registered event handler '{}' (total: {})",
            handler.name(),
            handlers.len()
        ));
        
        Ok(())
    }
    
    /// Event-Handler entfernen
    pub fn unregister_handler(&self, handler_name: &str) -> Result<()> {
        let mut handlers = self.handlers.write()
            .map_err(|e| anyhow::anyhow!("Failed to acquire write lock: {}", e))?;
        
        let initial_len = handlers.len();
        handlers.retain(|h| h.name() != handler_name);
        
        let removed = initial_len - handlers.len();
        if removed > 0 {
            self.info(&format!("Unregistered event handler '{}'", handler_name));
            Ok(())
        } else {
            anyhow::bail!("Handler '{}' not found", handler_name)
        }
    }
    
    /// Anzahl verarbeiteter Events
    pub fn event_count(&self) -> u64 {
        self.event_count.load(Ordering::Relaxed)
    }
    
    /// Liste der registrierten Handler
    pub fn handler_list(&self) -> Vec<String> {
        match self.handlers.read() {
            Ok(guard) => guard.iter().map(|h| h.name().to_string()).collect(),
            Err(_) => {
                reset_poisoned_handlers(&self.handlers, self, "handler_list");
                Vec::new()
            }
        }
    }
    
    fn processing_loop(
        event_rx: Receiver<Event>,
        handlers: Arc<RwLock<Vec<Arc<dyn EventHandler>>>>,
        running: Arc<AtomicBool>,
        name: String,
        event_count: std::sync::atomic::AtomicU64,
    ) {
        let bus_logger = EventBusLogger { name: name.clone() };
        bus_logger.info("EventBus processing thread started");
        
        let mut consecutive_errors = 0;
        let max_consecutive_errors = 10;
        
        while running.load(Ordering::Relaxed) {
            match event_rx.try_recv() {
                Ok(event) => {
                    consecutive_errors = 0;
                    
                    // Get handlers (with read lock)
                    let handlers_guard = match handlers.read() {
                        Ok(guard) => guard,
                        Err(_) => {
                            reset_poisoned_handlers(&handlers, &bus_logger, "processing_loop");
                            continue;
                        }
                    };
                    
                    // Distribute to handlers
                    for handler in handlers_guard.iter() {
                        // Check filters
                        if let Some(min_priority) = handler.priority_filter() {
                            if (event.priority as u8) < (min_priority as u8) {
                                continue;
                            }
                        }
                        
                        if let Some(allowed_types) = handler.event_type_filter() {
                            if !allowed_types.iter().any(|t| std::mem::discriminant(t) == std::mem::discriminant(&event.event_type)) {
                                continue;
                            }
                        }
                        
                        // Handle event
                        if let Err(e) = handler.handle_event(&event) {
                            bus_logger.error(&format!(
                                "Handler '{}' failed to process event {}: {}",
                                handler.name(),
                                event.id,
                                e
                            ));
                        }
                    }
                    
                    // Drop guard before sleep
                    drop(handlers_guard);
                }
                
                Err(TryRecvError::Empty) => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                
                Err(TryRecvError::Disconnected) => {
                    bus_logger.error("Event channel disconnected");
                    break;
                }
            }
            
            // Periodisches Logging alle 1000 Events
            let count = event_count.load(Ordering::Relaxed);
            if count > 0 && count % 1000 == 0 {
                bus_logger.info(&format!("Processed {} events", count));
            }
        }
        
        bus_logger.info("EventBus processing thread stopped");
    }
}

fn reset_poisoned_handlers<L: ComponentLogger>(
    handlers: &Arc<RwLock<Vec<Arc<dyn EventHandler>>>>,
    logger: &L,
    context: &str,
) {
    logger.error(&format!(
        "Handler lock poisoned in {}, resetting handler list",
        context
    ));
    match handlers.write() {
        Ok(mut guard) => {
            guard.clear();
        }
        Err(poisoned) => {
            let mut guard = poisoned.into_inner();
            guard.clear();
        }
    }
}

// Logger für den EventBus Processing Thread
struct EventBusLogger {
    name: String,
}

impl crate::core::logging::ComponentLogger for EventBusLogger {
    fn log_context(&self) -> crate::core::logging::LogContext {
        crate::core::logging::LogContext::new("EventBus", &self.name)
    }
}

// Implementierung des ComponentLogger Traits für EventBus
impl crate::core::logging::ComponentLogger for EventBus {
    fn log_context(&self) -> crate::core::logging::LogContext {
        crate::core::logging::LogContext::new("EventBus", &self.name)
    }
}

// Standard-Event-Handler

/// Event-Logger Handler (schreibt Events in Log)
pub struct EventLoggerHandler {
    name: String,
    min_priority: EventPriority,
}

impl EventLoggerHandler {
    pub fn new(name: &str, min_priority: EventPriority) -> Self {
        Self {
            name: name.to_string(),
            min_priority,
        }
    }
}

impl EventHandler for EventLoggerHandler {
    fn handle_event(&self, event: &Event) -> Result<()> {
        if (event.priority as u8) < (self.min_priority as u8) {
            return Ok(());
        }
        
        // Strukturierte Log-Ausgabe
        let log_message = format!(
            "[event_id={}][type={:?}] {}",
            event.id,
            event.event_type,
            event.payload_str()
        );
        
        match event.priority {
            EventPriority::Debug => log::debug!("{}", log_message),
            EventPriority::Info => log::info!("{}", log_message),
            EventPriority::Warning => log::warn!("{}", log_message),
            EventPriority::Error => log::error!("{}", log_message),
            EventPriority::Critical => log::error!("CRITICAL: {}", log_message),
        }
        
        Ok(())
    }
    
    fn name(&self) -> &str {
        &self.name
    }
    
    fn priority_filter(&self) -> Option<EventPriority> {
        Some(self.min_priority)
    }
}

/// Event-File-Handler (schreibt Events in Datei)
pub struct EventFileHandler {
    name: String,
    file_path: String,
    max_file_size: usize,
}

impl EventFileHandler {
    pub fn new(name: &str, file_path: &str, max_file_size: usize) -> Self {
        Self {
            name: name.to_string(),
            file_path: file_path.to_string(),
            max_file_size,
        }
    }
}

impl EventHandler for EventFileHandler {
    fn handle_event(&self, event: &Event) -> Result<()> {
        use std::fs::OpenOptions;
        use std::io::Write;
        
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file_path)?;
        
        let mut writer = std::io::BufWriter::new(file);
        writeln!(writer, "{}", event.to_json())?;
        writer.flush()?;
        
        Ok(())
    }
    
    fn name(&self) -> &str {
        &self.name
    }
}

/// Event-Stats-Handler (sammelt Statistiken)
pub struct EventStatsHandler {
    name: String,
    stats: std::sync::Mutex<EventHandlerStats>,
}

#[derive(Debug, Default)]
struct EventHandlerStats {
    total_events: u64,
    events_by_type: std::collections::HashMap<String, u64>,
    events_by_priority: std::collections::HashMap<EventPriority, u64>,
    last_event_time: Option<u64>,
}

impl EventStatsHandler {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            stats: std::sync::Mutex::new(EventHandlerStats::default()),
        }
    }
    
    pub fn get_stats(&self) -> Result<EventHandlerStats> {
        let stats = self.stats.lock()
            .map_err(|e| anyhow::anyhow!("Failed to lock stats: {}", e))?;
        Ok(stats.clone())
    }
}

impl EventHandler for EventStatsHandler {
    fn handle_event(&self, event: &Event) -> Result<()> {
        let mut stats = self.stats.lock()
            .map_err(|e| anyhow::anyhow!("Failed to lock stats: {}", e))?;
        
        stats.total_events += 1;
        *stats.events_by_type.entry(format!("{:?}", event.event_type))
            .or_insert(0) += 1;
        *stats.events_by_priority.entry(event.priority)
            .or_insert(0) += 1;
        stats.last_event_time = Some(event.timestamp);
        
        Ok(())
    }
    
    fn name(&self) -> &str {
        &self.name
    }
}

// Unit Tests
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::events::{EventBuilder, EventType, EventPriority};

    #[test]
    fn test_event_bus_creation() {
        let mut bus = EventBus::new("test_bus");
        assert_eq!(bus.name, "test_bus");
        assert_eq!(bus.event_count(), 0);
        
        bus.start().unwrap();
        bus.stop().unwrap();
    }

    #[test]
    fn test_event_publishing() {
        let mut bus = EventBus::new("test_pub");
        bus.start().unwrap();
        
        let builder = EventBuilder::new("test", "instance");
        let event = builder.buffer_created("test_buffer", 100);
        
        bus.publish(event).unwrap();
        
        // Wait for processing
        std::thread::sleep(std::time::Duration::from_millis(100));
        
        assert!(bus.event_count() > 0);
        bus.stop().unwrap();
    }

    #[test]
    fn test_event_handler_registration() {
        let bus = EventBus::new("test_handler");
        let handler = Arc::new(EventLoggerHandler::new("test_logger", EventPriority::Debug));
        
        bus.register_handler(handler).unwrap();
        assert_eq!(bus.handler_list(), vec!["test_logger"]);
    }

    #[test]
    fn test_event_builder() {
        let builder = EventBuilder::new("test", "instance");
        
        let event = builder.buffer_created("test", 100);
        assert_eq!(format!("{:?}", event.event_type), "BufferCreated");
        
        let error_event = builder.error("TestError", "Test message", None);
        assert_eq!(error_event.priority, EventPriority::Error);
    }
}
