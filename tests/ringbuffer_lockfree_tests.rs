#![cfg(feature = "lockfree")]

use airlift_node::{AudioRingBuffer, PcmFrame};

#[test]
fn test_multi_reader_lockfree() {
    let buffer = AudioRingBuffer::new(8);
    for i in 0..3 {
        buffer.push(PcmFrame {
            utc_ns: i as u64 * 1000,
            samples: vec![i as i16; 32],
            sample_rate: 48000,
            channels: 2,
        });
    }

    let r1_first = buffer.pop_for_reader("reader_one").unwrap();
    let r2_first = buffer.pop_for_reader("reader_two").unwrap();
    assert_eq!(r1_first.samples[0], 0);
    assert_eq!(r2_first.samples[0], 0);

    let r1_second = buffer.pop_for_reader("reader_one").unwrap();
    let r2_second = buffer.pop_for_reader("reader_two").unwrap();
    assert_eq!(r1_second.samples[0], 1);
    assert_eq!(r2_second.samples[0], 1);
}

#[test]
fn test_wrap_around_lockfree() {
    let buffer = AudioRingBuffer::new(3);
    for i in 0..6 {
        buffer.push(PcmFrame {
            utc_ns: i as u64 * 1000,
            samples: vec![i as i16; 16],
            sample_rate: 48000,
            channels: 2,
        });
    }

    assert_eq!(buffer.len(), 3);
    let stats = buffer.stats();
    assert!(stats.dropped_frames > 0);
}
