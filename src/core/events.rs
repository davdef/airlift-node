// src/core/events.rs
use serde::{Deserialize, Serialize};
use super::logging::LogContext;

/// Event-Typen im System
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventType {
    Error,
    BufferOverflow,
    ConfigChanged,
    #[cfg(feature = "debug-events")]
    Debug(DebugEventType),
}

#[cfg(feature = "debug-events")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DebugEventType {
    BufferCreated,
    ProducerStarted,
    FlowStatus,
    NodeStarted,
    NodeStopped,
    ProducerAdded,
}

/// Event-Priorität
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
            EventType::Error => "Error",
            EventType::BufferOverflow => "BufferOverflow",
            EventType::ConfigChanged => "ConfigChanged",
            #[cfg(feature = "debug-events")]
            EventType::Debug(debug_event) => debug_event.event_type_str(),
        }
    }
    
    pub fn payload_str(&self) -> String {
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
    
    pub fn error(&self, error_type: &str, message: &str, details: Option<serde_json::Value>) -> Event {
        Event::new(
            EventType::Error,
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

    pub fn buffer_overflow(&self, buffer_name: &str, capacity: usize, dropped: usize) -> Event {
        Event::new(
            EventType::BufferOverflow,
            EventPriority::Warning,
            &self.source,
            &self.source_instance,
            serde_json::json!({
                "buffer_name": buffer_name,
                "capacity": capacity,
                "dropped": dropped,
                "timestamp": crate::core::timestamp::utc_ns_now(),
            }),
        )
    }

    pub fn config_changed(&self, component: &str, changes: serde_json::Value) -> Event {
        Event::new(
            EventType::ConfigChanged,
            EventPriority::Info,
            &self.source,
            &self.source_instance,
            serde_json::json!({
                "component": component,
                "changes": changes,
                "timestamp": crate::core::timestamp::utc_ns_now(),
            }),
        )
    }
}

#[cfg(feature = "debug-events")]
impl DebugEventType {
    fn event_type_str(&self) -> &str {
        match self {
            DebugEventType::BufferCreated => "Debug.BufferCreated",
            DebugEventType::ProducerStarted => "Debug.ProducerStarted",
            DebugEventType::FlowStatus => "Debug.FlowStatus",
            DebugEventType::NodeStarted => "Debug.NodeStarted",
            DebugEventType::NodeStopped => "Debug.NodeStopped",
            DebugEventType::ProducerAdded => "Debug.ProducerAdded",
        }
    }
}

#[cfg(feature = "debug-events")]
impl EventBuilder {
    pub fn buffer_created(&self, name: &str, capacity: usize) -> Event {
        Event::new(
            EventType::Debug(DebugEventType::BufferCreated),
            EventPriority::Debug,
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
            EventType::Debug(DebugEventType::ProducerStarted),
            EventPriority::Debug,
            &self.source,
            &self.source_instance,
            serde_json::json!({
                "producer_name": name,
                "config": config,
                "timestamp": crate::core::timestamp::utc_ns_now(),
            }),
        )
    }

    pub fn flow_status(&self, flow_name: &str, status: serde_json::Value) -> Event {
        Event::new(
            EventType::Debug(DebugEventType::FlowStatus),
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
