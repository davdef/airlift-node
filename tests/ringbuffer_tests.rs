use airlift_node::{AudioRingBuffer, ComponentLogger, PcmFrame};

#[test]
fn test_basic_buffer_operations_debug() {
    let buffer = AudioRingBuffer::new(10);
    assert_eq!(buffer.len(), 0);
    assert_eq!(buffer.available(), 0);

    let frame = PcmFrame {
        utc_ns: 123456789,
        samples: vec![1, 2, 3, 4, 5, 6],
        sample_rate: 48000,
        channels: 2,
    };

    let new_len = buffer.push(frame);
    assert_eq!(new_len, 1);
    assert_eq!(buffer.len(), 1);
    assert_eq!(buffer.available(), 1);

    let popped = buffer.pop();
    assert!(popped.is_some());

    // Frame ist noch im Buffer, aber nicht mehr verfügbar für "default"
    assert_eq!(buffer.len(), 1);
    assert_eq!(buffer.available(), 0);

    // Für anderen Reader verfügbar
    assert_eq!(buffer.available_for_reader("other"), 1);
}

#[test]
fn test_basic_buffer_operations() {
    let buffer = AudioRingBuffer::new(10);
    assert_eq!(buffer.len(), 0);

    let frame = PcmFrame {
        utc_ns: 123456789,
        samples: vec![1, 2, 3, 4, 5, 6],
        sample_rate: 48000,
        channels: 2,
    };

    let new_len = buffer.push(frame);
    assert_eq!(new_len, 1);
    assert_eq!(buffer.len(), 1);

    // Reader "default" liest den Frame
    let popped = buffer.pop();
    assert!(popped.is_some());

    // Buffer hat immer noch 1 Frame (nur Leseposition geändert)
    assert_eq!(buffer.len(), 1);

    // Zweiter Versuch von "default" sollte nichts liefern
    let popped2 = buffer.pop();
    assert!(popped2.is_none());

    // Aber anderer Reader kann lesen
    let popped_by_other = buffer.pop_for_reader("other_reader");
    assert!(popped_by_other.is_some());

    // Jetzt haben beide Reader gelesen, Buffer immer noch da
    assert_eq!(buffer.len(), 1);
}

#[test]
fn test_multi_reader() {
    let buffer = AudioRingBuffer::new(20);

    // Push 3 frames
    for i in 0..3 {
        let frame = PcmFrame {
            utc_ns: i as u64 * 1000,
            samples: vec![i as i16; 96],
            sample_rate: 48000,
            channels: 2,
        };
        buffer.push(frame);
    }

    assert_eq!(buffer.len(), 3);

    // Two different readers should both get the first frame
    let frame1 = buffer.pop_for_reader("reader1");
    let frame2 = buffer.pop_for_reader("reader2");

    assert!(frame1.is_some());
    assert!(frame2.is_some());
    assert_eq!(frame1.unwrap().samples[0], 0);
    assert_eq!(frame2.unwrap().samples[0], 0);

    // Now reader1 should get frame 1, reader2 should get frame 2
    let frame1_2 = buffer.pop_for_reader("reader1");
    let frame2_2 = buffer.pop_for_reader("reader2");

    assert!(frame1_2.is_some());
    assert!(frame2_2.is_some());
    assert_eq!(frame1_2.unwrap().samples[0], 1);
    assert_eq!(frame2_2.unwrap().samples[0], 1);
}

#[test]
fn test_buffer_wrap_around() {
    let buffer = AudioRingBuffer::new(3);

    // Push mehr Frames als Capacity
    for i in 0..5 {
        let frame = PcmFrame {
            utc_ns: i as u64 * 1000,
            samples: vec![i as i16; 48],
            sample_rate: 48000,
            channels: 2,
        };
        buffer.push(frame);
    }

    // Should only have 3 frames (wrapped around)
    assert_eq!(buffer.len(), 3);

    let stats = buffer.stats();
    assert!(stats.dropped_frames > 0);
}

#[test]
fn test_buffer_logging_integration() {
    let buffer = AudioRingBuffer::new(5);

    // Teste, dass Logging-Methoden verfügbar sind
    buffer.debug("Test debug message");
    buffer.info("Test info message");
    buffer.warn("Test warning message");
    buffer.error("Test error message");

    // Teste buffer tracing
    buffer.trace_buffer(&buffer);

    // Fülle Buffer und trace
    for i in 0..3 {
        let frame = PcmFrame {
            utc_ns: i as u64 * 1000,
            samples: vec![i as i16; 48],
            sample_rate: 48000,
            channels: 2,
        };
        buffer.push(frame);
    }

    buffer.trace_buffer(&buffer);

    // Teste multi-reader mit logging
    let r1 = buffer.pop_for_reader("test_reader1");
    let r2 = buffer.pop_for_reader("test_reader2");

    assert!(r1.is_some());
    assert!(r2.is_some());

    buffer.trace_buffer(&buffer);
}
