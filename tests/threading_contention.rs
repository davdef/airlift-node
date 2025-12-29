use std::sync::{Arc, Barrier};
use std::sync::atomic::{AtomicUsize, Ordering};

use airlift_node::core::ringbuffer::{AudioRingBuffer, PcmFrame};

#[test]
fn test_ringbuffer_threading_contention() {
    let buffer = Arc::new(AudioRingBuffer::new(64));
    let start = Arc::new(Barrier::new(5));
    let pushed = Arc::new(AtomicUsize::new(0));
    let popped = Arc::new(AtomicUsize::new(0));

    let mut handles = Vec::new();

    for producer_id in 0..2 {
        let buffer = buffer.clone();
        let start = start.clone();
        let pushed = pushed.clone();
        handles.push(std::thread::spawn(move || {
            start.wait();
            for i in 0..200 {
                let frame = PcmFrame {
                    utc_ns: (producer_id * 1_000 + i) as u64,
                    samples: vec![producer_id as i16; 32],
                    sample_rate: 48_000,
                    channels: 2,
                };
                buffer.push(frame);
                pushed.fetch_add(1, Ordering::Relaxed);
                if i % 50 == 0 {
                    std::thread::yield_now();
                }
            }
        }));
    }

    for reader_id in 0..2 {
        let buffer = buffer.clone();
        let start = start.clone();
        let popped = popped.clone();
        handles.push(std::thread::spawn(move || {
            start.wait();
            for _ in 0..400 {
                if buffer.pop_for_reader(&format!("reader-{}", reader_id)).is_some() {
                    popped.fetch_add(1, Ordering::Relaxed);
                } else {
                    std::thread::yield_now();
                }
            }
        }));
    }

    start.wait();

    for handle in handles {
        handle.join().expect("thread should complete");
    }

    assert!(pushed.load(Ordering::Relaxed) >= 400);
    assert!(popped.load(Ordering::Relaxed) >= 1);
    assert!(buffer.len() <= 64);
}
