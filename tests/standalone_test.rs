// tests/standalone_test.rs
// Kompiliert als eigenes Binary mit: cargo build --test standalone_test
// ODER: cargo test --test standalone_test

fn main() {
    println!("=== Standalone Airlift Tests ===");

    test_core_components();
    test_logging_system();

    println!("\n✅ All standalone tests completed!");
}

fn test_core_components() {
    println!("\n--- Testing Core Components ---");

    use airlift_node::core::ringbuffer::AudioRingBuffer;

    // Test 1: Basic buffer
    let buffer = AudioRingBuffer::new(5);
    println!(
        "✓ Created AudioRingBuffer with capacity {}",
        buffer.stats().capacity
    );

    // Test 2: Push/Pop
    let frame = airlift_node::core::ringbuffer::PcmFrame {
        utc_ns: 123456789,
        samples: vec![42, 43, 44],
        sample_rate: 48000,
        channels: 1,
    };

    buffer.push(frame);
    println!("✓ Pushed frame to buffer");

    if let Some(pop_frame) = buffer.pop() {
        println!("✓ Popped frame with {} samples", pop_frame.samples.len());
    }

    // Test 3: Multi-reader
    let buffer = AudioRingBuffer::new(10);
    for i in 0..3 {
        let frame = airlift_node::core::ringbuffer::PcmFrame {
            utc_ns: i as u64 * 1000,
            samples: vec![i as i16; 96],
            sample_rate: 48000,
            channels: 2,
        };
        buffer.push(frame);
    }

    println!("✓ Pushed 3 frames for multi-reader test");

    let r1 = buffer.pop_for_reader("reader_1");
    let r2 = buffer.pop_for_reader("reader_2");

    println!("✓ Reader 1 got frame: {}", r1.is_some());
    println!("✓ Reader 2 got frame: {}", r2.is_some());
}

fn test_logging_system() {
    println!("\n--- Testing Logging System ---");

    use airlift_node::core::logging::{ComponentLogger, LogContext};

    struct TestComponent {
        id: String,
    }

    impl TestComponent {
        fn new(id: &str) -> Self {
            Self { id: id.to_string() }
        }
    }

    impl ComponentLogger for TestComponent {
        fn log_context(&self) -> LogContext {
            LogContext::new("TestComponent", &self.id)
        }
    }

    let component = TestComponent::new("comp_001");

    // Just create context to verify it works
    let ctx = component.log_context();
    println!("✓ Created LogContext for {}", ctx.component);
    println!(
        "  Sequence: {}, Timestamp: {} ns",
        ctx.sequence, ctx.timestamp_ns
    );

    let formatted = ctx.format("DEBUG", "Test log message");
    println!("✓ Formatted log message: {}", formatted);
}
