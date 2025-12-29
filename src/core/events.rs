// src/core/events.rs
use serde::{Deserialize, Serialize};
use std::time::SystemTime;
use super::logging::LogContext;

/// Event-Typen im System
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventType {
    // Buffer-Events
    BufferCreated,
    BufferUpdated,
    BufferRemoved,
    BufferOverflow,
    
    // Producer-Events
    ProducerStarted,
    ProducerStopped,
    ProducerError,
    ProducerDisconnected,
    
    // Consumer-Events
    ConsumerStarted,
    ConsumerStopped,
    ConsumerError,
    ConsumerConnected,
    
    // Flow-Events
    FlowCreated,
    FlowStarted,
    FlowStopped,
    FlowError,
    FlowStatistics,
    
    // Processor-Events
    ProcessorAdded,
    ProcessorRemoved,
    ProcessorError,
    ProcessorConfigChanged,
    
    // Node-Events
    NodeStarted,
    NodeStopped,
    NodeConfigChanged,
    NodeStatusUpdate,
    
    // System-Events
    DeviceDiscovered,
    DeviceTested,
    SignalDetected,
    Warning,
    CriticalError,
}

/// Event-Priorität
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventPriority {
    Debug,
    Info,
    Warning,
    Error,
    Critical,
}

/// Event-Struktur mit Metadaten
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: u64,
    pub timestamp: u64,
    pub event_type: EventType,
    pub priority: EventPriority,
    pub source: String,
    pub source_instance: String,
    pub payload: serde_json::Value,
    pub context: Option<LogContext>,
    pub correlation_id: Option<String>,
}

impl Event {
    pub fn new(
        event_type: EventType,
        priority: EventPriority,
        source: &str,
        source_instance: &str,
        payload: serde_json::Value,
    ) -> Self {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        
        Self {
            id: COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst),
            timestamp: crate::core::timestamp::utc_ns_now(),
            event_type,
            priority,
            source: source.to_string(),
            source_instance: source_instance.to_string(),
            payload,
            context: None,
            correlation_id: None,
        }
    }
    
    pub fn with_context(mut self, context: LogContext) -> Self {
        self.context = Some(context);
        self
    }
    
    pub fn with_correlation(mut self, correlation_id: &str) -> Self {
        self.correlation_id = Some(correlation_id.to_string());
        self
    }
    
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }
    
    pub fn format_message(&self) -> String {
        let corr_info = match &self.correlation_id {
            Some(cid) => format!("[corr={}] ", cid),
            None => String::new(),
        };
        
        format!(
            "[{}][{}] {}{}: {}",
            self.priority_str(),
            self.source,
            corr_info,
            self.event_type_str(),
            self.payload_str()
        )
    }
    
    fn priority_str(&self) -> &str {
        match self.priority {
            EventPriority::Debug => "DEBUG",
            EventPriority::Info => "INFO",
            EventPriority::Warning => "WARN",
            EventPriority::Error => "ERROR",
            EventPriority::Critical => "CRITICAL",
        }
    }
    
    fn event_type_str(&self) -> &str {
        match &self.event_type {
            EventType::BufferCreated => "BufferCreated",
            EventType::BufferUpdated => "BufferUpdated",
            EventType::BufferRemoved => "BufferRemoved",
            EventType::BufferOverflow => "BufferOverflow",
            EventType::ProducerStarted => "ProducerStarted",
            EventType::ProducerStopped => "ProducerStopped",
            EventType::ProducerError => "ProducerError",
            EventType::ProducerDisconnected => "ProducerDisconnected",
            EventType::ConsumerStarted => "ConsumerStarted",
            EventType::ConsumerStopped => "ConsumerStopped",
            EventType::ConsumerError => "ConsumerError",
            EventType::ConsumerConnected => "ConsumerConnected",
            EventType::FlowCreated => "FlowCreated",
            EventType::FlowStarted => "FlowStarted",
            EventType::FlowStopped => "FlowStopped",
            EventType::FlowError => "FlowError",
            EventType::FlowStatistics => "FlowStatistics",
            EventType::ProcessorAdded => "ProcessorAdded",
            EventType::ProcessorRemoved => "ProcessorRemoved",
            EventType::ProcessorError => "ProcessorError",
            EventType::ProcessorConfigChanged => "ProcessorConfigChanged",
            EventType::NodeStarted => "NodeStarted",
            EventType::NodeStopped => "NodeStopped",
            EventType::NodeConfigChanged => "NodeConfigChanged",
            EventType::NodeStatusUpdate => "NodeStatusUpdate",
            EventType::DeviceDiscovered => "DeviceDiscovered",
            EventType::DeviceTested => "DeviceTested",
            EventType::SignalDetected => "SignalDetected",
            EventType::Warning => "Warning",
            EventType::CriticalError => "CriticalError",
        }
    }
    
    fn payload_str(&self) -> String {
        match &self.payload {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Null => "null".to_string(),
            _ => self.payload.to_string(),
        }
    }
}

// Event-Builder für häufige Event-Typen
pub struct EventBuilder {
    source: String,
    source_instance: String,
}

impl EventBuilder {
    pub fn new(source: &str, source_instance: &str) -> Self {
        Self {
            source: source.to_string(),
            source_instance: source_instance.to_string(),
        }
    }
    
    pub fn buffer_created(&self, name: &str, capacity: usize) -> Event {
        Event::new(
            EventType::BufferCreated,
            EventPriority::Info,
            &self.source,
            &self.source_instance,
            serde_json::json!({
                "buffer_name": name,
                "capacity": capacity,
                "timestamp": crate::core::timestamp::utc_ns_now(),
            }),
        )
    }
    
    pub fn producer_started(&self, name: &str, config: serde_json::Value) -> Event {
        Event::new(
            EventType::ProducerStarted,
            EventPriority::Info,
            &self.source,
            &self.source_instance,
            serde_json::json!({
                "producer_name": name,
                "config": config,
                "timestamp": crate::core::timestamp::utc_ns_now(),
            }),
        )
    }
    
    pub fn error(&self, error_type: &str, message: &str, details: Option<serde_json::Value>) -> Event {
        Event::new(
            EventType::CriticalError,
            EventPriority::Error,
            &self.source,
            &self.source_instance,
            serde_json::json!({
                "error_type": error_type,
                "message": message,
                "details": details,
                "timestamp": crate::core::timestamp::utc_ns_now(),
            }),
        )
    }
    
    pub fn flow_status(&self, flow_name: &str, status: serde_json::Value) -> Event {
        Event::new(
            EventType::FlowStatistics,
            EventPriority::Debug,
            &self.source,
            &self.source_instance,
            serde_json::json!({
                "flow_name": flow_name,
                "status": status,
                "timestamp": crate::core::timestamp::utc_ns_now(),
            }),
        )
    }
}
