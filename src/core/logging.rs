// src/core/logging.rs - Vereinfachte Version
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

// Globale Sequenznummer für Korrelation
static LOG_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct LogContext {
    pub component: String,
    pub instance_id: String,
    pub flow_id: Option<String>,
    pub sequence: u64,
    pub timestamp_ns: u64,
}

impl LogContext {
    pub fn new(component: &str, instance_id: &str) -> Self {
        Self {
            component: component.to_string(),
            instance_id: instance_id.to_string(),
            flow_id: None,
            sequence: LOG_SEQUENCE.fetch_add(1, Ordering::Relaxed),
            timestamp_ns: utc_ns_now(),
        }
    }
    
    pub fn with_flow(mut self, flow_id: &str) -> Self {
        self.flow_id = Some(flow_id.to_string());
        self
    }
    
    pub fn format(&self, level: &str, message: &str) -> String {
        let flow_info = match &self.flow_id {
            Some(flow) => format!(" flow={}", flow),
            None => String::new(),
        };
        
        format!(
            "[{}][seq={:06}][{}:{}{}] {}",
            level,
            self.sequence,
            self.component,
            self.instance_id,
            flow_info,
            message
        )
    }
}

// Helper Trait für einheitliches Logging
pub trait ComponentLogger {
    fn log_context(&self) -> LogContext;
    
    fn debug(&self, message: &str) {
        let ctx = self.log_context();
        log::debug!("{}", ctx.format("DEBUG", message));
    }
    
    fn info(&self, message: &str) {
        let ctx = self.log_context();
        log::info!("{}", ctx.format("INFO", message));
    }
    
    fn warn(&self, message: &str) {
        let ctx = self.log_context();
        log::warn!("{}", ctx.format("WARN", message));
    }
    
    fn error(&self, message: &str) {
        let ctx = self.log_context();
        log::error!("{}", ctx.format("ERROR", message));
    }
    
    fn trace_buffer(&self, buffer: &super::ringbuffer::AudioRingBuffer) {
        let stats = buffer.stats();
        let ctx = self.log_context();
        
        let buffer_info = format!(
            "buffer[addr={:?}] frames={}/{} dropped={}",
            buffer as *const _,
            stats.current_frames,
            stats.capacity,
            stats.dropped_frames
        );
        
        log::debug!("{}", ctx.format("TRACE", &buffer_info));
    }
}

// Utils
pub fn utc_ns_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64
}

// Am ENDE von src/core/logging.rs

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_context_creation() {
        let ctx = LogContext::new("Producer", "alsa:default");
        
        assert_eq!(ctx.component, "Producer");
        assert_eq!(ctx.instance_id, "alsa:default");
        assert!(ctx.sequence > 0);
        assert!(ctx.timestamp_ns > 0);
        assert!(ctx.flow_id.is_none());
    }

    #[test]
    fn test_log_context_with_flow() {
        let ctx = LogContext::new("Flow", "main")
            .with_flow("recording_flow");
            
        assert_eq!(ctx.flow_id, Some("recording_flow".to_string()));
    }

    #[test]
    fn test_log_formatting() {
        let ctx = LogContext::new("Test", "001");
        let formatted = ctx.format("INFO", "Starting up");
        
        assert!(formatted.contains("[INFO]"));
        assert!(formatted.contains("[Test:001]"));
        assert!(formatted.contains("Starting up"));
        
        // Mit Flow
        let ctx_with_flow = ctx.with_flow("main_flow");
        let formatted_with_flow = ctx_with_flow.format("DEBUG", "Processing");
        
        assert!(formatted_with_flow.contains("flow=main_flow"));
    }

    #[test]
    fn test_component_logger_trait() {
        struct MockComponent {
            id: String,
        }
        
        impl MockComponent {
            fn new(id: &str) -> Self {
                Self { id: id.to_string() }
            }
        }
        
        impl ComponentLogger for MockComponent {
            fn log_context(&self) -> LogContext {
                LogContext::new("Mock", &self.id)
            }
        }
        
        let component = MockComponent::new("test_001");
        let ctx = component.log_context();
        
        assert_eq!(ctx.component, "Mock");
        assert_eq!(ctx.instance_id, "test_001");
    }
}
