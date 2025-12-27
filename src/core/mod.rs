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
