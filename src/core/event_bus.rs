// src/core/event_bus.rs

use super::events::{Event, EventPriority, EventType};
use super::lock::{lock_mutex, lock_rwlock_read, lock_rwlock_write};
use super::logging::{ComponentLogger, LogContext};

use anyhow::Result;
use crossbeam_channel::{select, unbounded, Receiver, Sender};

use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, RwLock,
};

/// Event-Handler Trait
pub trait EventHandler: Send + Sync {
    fn handle_event(&self, event: &Event) -> Result<()>;
    fn name(&self) -> &str;

    fn priority_filter(&self) -> Option<EventPriority> {
        None
    }

    fn event_type_filter(&self) -> Option<Vec<EventType>> {
        None
    }
}

/// Event-Bus
pub struct EventBus {
    name: String,

    event_tx: Sender<Event>,
    event_rx: Receiver<Event>,

    stop_tx: Sender<()>,
    stop_rx: Receiver<()>,

    handlers: Arc<RwLock<Vec<Arc<dyn EventHandler>>>>,

    running: Arc<AtomicBool>,
    event_count: Arc<AtomicU64>,

    thread_handle: Option<std::thread::JoinHandle<()>>,
}

impl EventBus {
    pub fn new(name: &str) -> Self {
        let (event_tx, event_rx) = unbounded();
        let (stop_tx, stop_rx) = unbounded();

        let bus = Self {
            name: name.to_string(),
            event_tx,
            event_rx,
            stop_tx,
            stop_rx,
            handlers: Arc::new(RwLock::new(Vec::new())),
            running: Arc::new(AtomicBool::new(false)),
            event_count: Arc::new(AtomicU64::new(0)),
            thread_handle: None,
        };

        bus.info(&format!("EventBus '{}' created", name));
        bus
    }

    /// Startet den Processing-Thread
    pub fn start(&mut self) -> Result<()> {
        if self.running.swap(true, Ordering::SeqCst) {
            return Ok(());
        }

        let event_rx = self.event_rx.clone();
        let stop_rx = self.stop_rx.clone();
        let handlers = self.handlers.clone();
        let running = self.running.clone();
        let event_count = self.event_count.clone();
        let name = self.name.clone();

        let handle = std::thread::spawn(move || {
            processing_loop(
                name,
                event_rx,
                stop_rx,
                handlers,
                running,
                event_count,
            );
        });

        self.thread_handle = Some(handle);
        self.info("EventBus started");
        Ok(())
    }

    /// Stoppt den Event-Bus
    pub fn stop(&mut self) -> Result<()> {
        if !self.running.swap(false, Ordering::SeqCst) {
            return Ok(());
        }

        let _ = self.stop_tx.send(());

        if let Some(handle) = self.thread_handle.take() {
            if let Err(e) = handle.join() {
                self.error(&format!("EventBus thread join failed: {:?}", e));
            }
        }

        self.info("EventBus stopped");
        Ok(())
    }

    /// Event publizieren
    pub fn publish(&self, event: Event) -> Result<()> {
        self.event_count.fetch_add(1, Ordering::Relaxed);

        // lokales Logging nach Priorität
        match event.priority {
            EventPriority::Critical | EventPriority::Error => {
                self.error(&event.format_message());
            }
            EventPriority::Warning => {
                self.warn(&event.format_message());
            }
            EventPriority::Info => {
                self.info(&event.format_message());
            }
            EventPriority::Debug => {
                self.debug(&event.format_message());
            }
        }

        self.event_tx.send(event)?;
        Ok(())
    }

    /// Handler registrieren
    pub fn register_handler(&self, handler: Arc<dyn EventHandler>) -> Result<()> {
        let mut handlers = lock_rwlock_write(&self.handlers, "event_bus.register_handler");
        handlers.push(handler.clone());

        self.info(&format!(
            "Registered handler '{}' (total={})",
            handler.name(),
            handlers.len()
        ));
        Ok(())
    }

    /// Handler entfernen
    pub fn unregister_handler(&self, handler_name: &str) -> Result<()> {
        let mut handlers = lock_rwlock_write(&self.handlers, "event_bus.unregister_handler");

        let before = handlers.len();
        handlers.retain(|h: &Arc<dyn EventHandler>| h.name() != handler_name);

        if handlers.len() == before {
            anyhow::bail!("Handler '{}' not found", handler_name);
        }

        self.info(&format!("Unregistered handler '{}'", handler_name));
        Ok(())
    }

    pub fn handler_list(&self) -> Vec<String> {
        let handlers = lock_rwlock_read(&self.handlers, "event_bus.handler_list");
        handlers
            .iter()
            .map(|h: &Arc<dyn EventHandler>| h.name().to_string())
            .collect()
    }

    pub fn event_count(&self) -> u64 {
        self.event_count.load(Ordering::Relaxed)
    }
}

/// ==========================
/// Processing Loop (Thread)
/// ==========================

fn processing_loop(
    name: String,
    event_rx: Receiver<Event>,
    stop_rx: Receiver<()>,
    handlers: Arc<RwLock<Vec<Arc<dyn EventHandler>>>>,
    running: Arc<AtomicBool>,
    event_count: Arc<AtomicU64>,
) {
    let logger = EventBusLogger { name };

    logger.info("EventBus processing loop started");

    while running.load(Ordering::Relaxed) {
        select! {
            recv(stop_rx) -> _ => {
                break;
            }
            recv(event_rx) -> msg => {
                let event = match msg {
                    Ok(e) => e,
                    Err(_) => break,
                };

                let handlers_guard =
                    lock_rwlock_read(&handlers, "event_bus.processing_loop");

                for handler in handlers_guard.iter() {

                    // Priority-Filter

let min_priority: Option<EventPriority> =
    EventHandler::priority_filter(&**handler);

if let Some(min) = min_priority {
    if event.priority < min {
        continue;
    }
}

                    // Event-Type-Filter

let allowed = EventHandler::event_type_filter(&**handler);

if let Some(allowed) = allowed {
    let matches = allowed.iter().any(|t| {
        std::mem::discriminant(t)
            == std::mem::discriminant(&event.event_type)
    });
    if !matches {
        continue;
    }
}

                    if let Err(e) = handler.handle_event(&event) {
                        logger.error(&format!(
                            "Handler '{}' failed for event {}: {}",
                            handler.name(),
                            event.id,
                            e
                        ));
                    }
                }
            }
        }

        let count = event_count.load(Ordering::Relaxed);
        if count > 0 && count % 1000 == 0 {
            logger.info(&format!("Processed {} events", count));
        }
    }

    logger.info("EventBus processing loop stopped");
}

/// ==========================
/// Logging
/// ==========================

struct EventBusLogger {
    name: String,
}

impl ComponentLogger for EventBusLogger {
    fn log_context(&self) -> LogContext {
        LogContext::new("EventBus", &self.name)
    }
}

impl ComponentLogger for EventBus {
    fn log_context(&self) -> LogContext {
        LogContext::new("EventBus", &self.name)
    }
}

/// ==========================
/// Standard-Handler
/// ==========================

#[derive(Debug, Default, Clone)]
pub struct EventHandlerStats {
    pub total_events: u64,
    pub events_by_type: HashMap<String, u64>,
    pub events_by_priority: HashMap<EventPriority, u64>,
    pub last_event_time: Option<u64>,
}

/// Audit-Handler
pub struct EventAuditHandler {
    name: String,
    min_priority: EventPriority,
    log_enabled: bool,
    stats: std::sync::Mutex<EventHandlerStats>,
}

impl EventAuditHandler {
    pub fn new(name: &str, min_priority: EventPriority) -> Self {
        Self {
            name: name.to_string(),
            min_priority,
            log_enabled: false,
            stats: std::sync::Mutex::new(EventHandlerStats::default()),
        }
    }

    pub fn with_logging(mut self, enabled: bool) -> Self {
        self.log_enabled = enabled;
        self
    }

    pub fn stats(&self) -> EventHandlerStats {
        lock_mutex(&self.stats, "event_audit_handler.stats").clone()
    }
}

impl EventHandler for EventAuditHandler {
    fn handle_event(&self, event: &Event) -> Result<()> {
        if event.priority < self.min_priority {
            return Ok(());
        }

        if self.log_enabled {
            log::info!(
                "[event_id={}][{:?}] {}",
                event.id,
                event.event_type,
                event.payload
            );
        }

        let mut stats = lock_mutex(&self.stats, "event_audit_handler.handle_event");
        stats.total_events += 1;
        *stats
            .events_by_type
            .entry(format!("{:?}", event.event_type))
            .or_insert(0) += 1;
        *stats.events_by_priority.entry(event.priority).or_insert(0) += 1;
        stats.last_event_time = Some(event.timestamp);

        Ok(())
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn priority_filter(&self) -> Option<EventPriority> {
        Some(self.min_priority)
    }
}

