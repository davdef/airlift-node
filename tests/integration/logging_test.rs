// tests/integration/logging_test.rs
use airlift_node::core::{AirliftNode, Flow, ComponentLogger};
use airlift_node::core::processor::basic::PassThrough;
use airlift_node::core::consumer::file_writer::FileConsumer;
use std::sync::Arc;

struct TestLogger {
    name: String,
}

impl TestLogger {
    fn new(name: &str) -> Self {
        Self { name: name.to_string() }
    }
}

impl ComponentLogger for TestLogger {
    fn log_context(&self) -> LogContext {
        LogContext::new("Test", &self.name)
    }
}

#[test]
fn test_basic_logging() {
    let logger = TestLogger::new("integration_test");
    logger.info("Starting integration test");
    
    let mut node = AirliftNode::new();
    logger.info(&format!("Node created: {:?}", node as *const _));
    
    // Test mit minimalem Setup
    let mut flow = Flow::new("test_flow");
    flow.add_processor(Box::new(PassThrough::new("test_passthrough")));
    
    // FileConsumer (aber ohne wirklich zu schreiben)
    let consumer = Box::new(FileConsumer::new("test_consumer", "/tmp/test.wav"));
    flow.add_consumer(consumer);
    
    logger.info("Test components created");
    assert!(true); // Platzhalter
}

#[test]
fn test_buffer_tracking() {
    use airlift_node::core::ringbuffer::AudioRingBuffer;
    
    let logger = TestLogger::new("buffer_test");
    let buffer = AudioRingBuffer::new(100);
    
    logger.trace_buffer(&buffer);
    
    // Buffer füllen
    for i in 0..5 {
        let frame = crate::core::ringbuffer::PcmFrame {
            utc_ns: logging::utc_ns_now(),
            samples: vec![i as i16; 480 * 2], // 480 Frames Stereo
            sample_rate: 48000,
            channels: 2,
        };
        buffer.push(frame);
        
        if i % 2 == 0 {
            logger.trace_buffer(&buffer);
        }
    }
    
    let stats = buffer.stats();
    logger.info(&format!("Final buffer stats: {:?}", stats));
    assert_eq!(stats.current_frames, 5);
}
