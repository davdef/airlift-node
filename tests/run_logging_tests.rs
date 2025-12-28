// tests/run_logging_tests.rs
fn main() {
    // Setup logging für Tests
    env_logger::Builder::from_env(
        env_logger::Env::default()
            .default_filter_or("debug")
    )
    .format_timestamp_micros()
    .format_module_path(false)
    .init();
    
    println!("=== Airlift Logging Tests ===");
    
    // Einfache Komponenten-Tests
    test_buffer_logging();
    test_flow_logging();
    test_multi_reader_logging();
}

fn test_buffer_logging() {
    use airlift_node::core::ringbuffer::AudioRingBuffer;
    
    println!("\n--- Test 1: Buffer Logging ---");
    let buffer = AudioRingBuffer::new(50);
    
    buffer.info("Buffer created");
    buffer.trace_buffer(&buffer);
    
    // Simuliere Produzenten
    for i in 0..25 {
        let frame = airlift_node::core::ringbuffer::PcmFrame {
            utc_ns: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64,
            samples: vec![i as i16; 96], // Kleine Frames
            sample_rate: 48000,
            channels: 2,
        };
        buffer.push(frame);
    }
    
    buffer.trace_buffer(&buffer);
    
    // Simuliere zwei Leser
    if let Some(frame) = buffer.pop_for_reader("reader_1") {
        buffer.debug(&format!("Reader 1 got frame with {} samples", frame.samples.len()));
    }
    
    if let Some(frame) = buffer.pop_for_reader("reader_2") {
        buffer.debug(&format!("Reader 2 got frame with {} samples", frame.samples.len()));
    }
    
    buffer.trace_buffer(&buffer);
}

fn test_flow_logging() {
    println!("\n--- Test 2: Flow Logging ---");
    
    use airlift_node::core::{AirliftNode, Flow};
    use airlift_node::core::processor::basic::PassThrough;
    use airlift_node::core::logging::ComponentLogger;
    
    struct FlowLogger {
        name: String,
    }
    
    impl ComponentLogger for FlowLogger {
        fn log_context(&self) -> LogContext {
            LogContext::new("TestFlow", &self.name)
        }
    }
    
    let logger = FlowLogger::new("test_flow_logging");
    logger.info("Starting flow logging test");
    
    // Hier später echte Flow-Tests
}

fn test_multi_reader_logging() {
    println!("\n--- Test 3: Multi-Reader Logging ---");
    
    use airlift_node::core::ringbuffer::AudioRingBuffer;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;
    
    let buffer = Arc::new(AudioRingBuffer::new(100));
    
    // Produzenten-Thread
    let producer_buffer = buffer.clone();
    let producer = thread::spawn(move || {
        for i in 0..20 {
            let frame = airlift_node::core::ringbuffer::PcmFrame {
                utc_ns: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos() as u64,
                samples: vec![i as i16; 192],
                sample_rate: 48000,
                channels: 2,
            };
            producer_buffer.push(frame);
            thread::sleep(Duration::from_millis(10));
        }
    });
    
    // Zwei Leser-Threads
    let reader1_buffer = buffer.clone();
    let reader1 = thread::spawn(move || {
        let mut count = 0;
        while count < 10 {
            if let Some(frame) = reader1_buffer.pop_for_reader("reader_1") {
                println!("[Reader1] Got frame {}: {} samples", count, frame.samples.len());
                count += 1;
            }
            thread::sleep(Duration::from_millis(15));
        }
    });
    
    let reader2_buffer = buffer.clone();
    let reader2 = thread::spawn(move || {
        let mut count = 0;
        while count < 10 {
            if let Some(frame) = reader2_buffer.pop_for_reader("reader_2") {
                println!("[Reader2] Got frame {}: {} samples", count, frame.samples.len());
                count += 1;
            }
            thread::sleep(Duration::from_millis(20));
        }
    });
    
    producer.join().unwrap();
    reader1.join().unwrap();
    reader2.join().unwrap();
    
    buffer.trace_buffer(&buffer);
}
