// tests/integration_tests.rs
// Integration tests laufen als externe Crate

#[test]
fn test_ringbuffer_basic() {
    use airlift_node::core::ringbuffer::AudioRingBuffer;
    
    let buffer = AudioRingBuffer::new(10);
    assert_eq!(buffer.len(), 0);
    
    // Push a frame
    let frame = airlift_node::core::ringbuffer::PcmFrame {
        utc_ns: 123456789,
        samples: vec![1, 2, 3, 4],
        sample_rate: 48000,
        channels: 2,
    };
    
    let new_len = buffer.push(frame);
    assert_eq!(new_len, 1);
    assert_eq!(buffer.len(), 1);
}

#[test]
fn test_ringbuffer_multi_reader() {
    use airlift_node::core::ringbuffer::AudioRingBuffer;
    
    let buffer = AudioRingBuffer::new(20);
    
    // Push two frames
    for i in 0..2 {
        let frame = airlift_node::core::ringbuffer::PcmFrame {
            utc_ns: i as u64 * 1000,
            samples: vec![i as i16; 48],
            sample_rate: 48000,
            channels: 2,
        };
        buffer.push(frame);
    }
    
    // Two different readers
    let frame1 = buffer.pop_for_reader("reader1");
    let frame2 = buffer.pop_for_reader("reader2");
    
    assert!(frame1.is_some());
    assert!(frame2.is_some());
    assert_eq!(frame1.unwrap().samples[0], 0);
    assert_eq!(frame2.unwrap().samples[0], 0);
}

#[test]
fn test_logging_context() {
    use airlift_node::core::logging::{LogContext, ComponentLogger};
    
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
            LogContext::new("TestComponent", &self.name)
        }
    }
    
    let logger = TestLogger::new("test_instance");
    let ctx = logger.log_context();
    
    let formatted = ctx.format("INFO", "Test message");
    assert!(formatted.contains("[INFO]"));
    assert!(formatted.contains("[TestComponent:test_instance]"));
    assert!(formatted.contains("Test message"));
}
