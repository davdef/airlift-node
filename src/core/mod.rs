pub mod buffer_registry;
pub mod connectable;
pub mod consumer;
pub mod device_scanner;
pub mod error;
pub mod event_bus;
pub mod events;
pub mod graph;
pub mod graph_api;
pub mod lock;
pub mod node;
pub mod plugin;
pub mod processor;
#[cfg(feature = "lockfree")]
#[path = "ringbuffer_lockfree.rs"]
pub mod ringbuffer;
#[cfg(not(feature = "lockfree"))]
pub mod ringbuffer;
pub mod timestamp;

pub use buffer_registry::BufferRegistry;
pub use consumer::{Consumer, ConsumerStatus};
pub use error::{AudioError, AudioResult, ConfigError};
pub use event_bus::{
    EventAuditHandler, EventBus, EventHandler, EventHandlerStats,
};
#[cfg(feature = "debug-events")]
pub use events::DebugEventType;
pub use events::{Event, EventBuilder, EventPriority, EventType};
pub use graph::{AudioGraph, GraphNode, GraphSnapshot, NodeClass};
pub use graph_api::{ConnectionRequest, DisconnectStrategy, GraphApi, NodeRequest};
pub use node::{AirliftNode, Flow};
pub use plugin::{AudioPlugin, PluginFactory, PluginInfo, ProcessorPluginAdapter};
pub use ringbuffer::*;
pub use timestamp::*;

pub trait Producer: Send + Sync {
    fn name(&self) -> &str;
    fn start(&mut self) -> anyhow::Result<()>;
    fn stop(&mut self) -> anyhow::Result<()>;
    fn status(&self) -> ProducerStatus;
    fn attach_ring_buffer(&mut self, buffer: std::sync::Arc<AudioRingBuffer>);
    fn attach_decoder(&mut self, _decoder: Box<dyn crate::decoders::AudioDecoder>) {}
}

#[derive(Debug, Clone)]
pub struct ProducerStatus {
    pub running: bool,
    pub connected: bool,
    pub samples_processed: u64,
    pub errors: u64,
    pub buffer_stats: Option<RingBufferStats>,
}

pub mod logging;
pub use logging::{ComponentLogger, LogContext};

// Am ENDE von src/core/mod.rs

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_producer_status() {
        let status = ProducerStatus {
            running: true,
            connected: true,
            samples_processed: 1000,
            errors: 0,
            buffer_stats: None,
        };

        assert!(status.running);
        assert!(status.connected);
        assert_eq!(status.samples_processed, 1000);
    }
}
