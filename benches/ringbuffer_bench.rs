use std::time::Instant;

use airlift_node::core::ringbuffer::AudioRingBuffer;
use airlift_node::PcmFrame;

fn main() {
    let buffer = AudioRingBuffer::new(1024);
    let frame = PcmFrame {
        utc_ns: 0,
        samples: vec![1; 96],
        sample_rate: 48_000,
        channels: 2,
    };

    let iterations = 512;
    let start = Instant::now();
    for _ in 0..iterations {
        buffer.push(frame.clone());
    }

    let mut consumed = 0;
    while let Some(_) = buffer.pop_for_reader("bench_reader") {
        consumed += 1;
        if consumed >= iterations {
            break;
        }
    }
    let elapsed = start.elapsed();

    println!(
        "Ringbuffer benchmark: {} pushes/pops in {:.2?} ({:.0} ops/s)",
        iterations,
        elapsed,
        iterations as f64 / elapsed.as_secs_f64()
    );
}
