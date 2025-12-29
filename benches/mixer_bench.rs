use std::sync::Arc;
use std::time::Instant;

use airlift_node::core::processor::Processor;
use airlift_node::core::ringbuffer::AudioRingBuffer;
use airlift_node::processors::Mixer;
use airlift_node::PcmFrame;

fn main() {
    let input_a = Arc::new(AudioRingBuffer::new(1024));
    let input_b = Arc::new(AudioRingBuffer::new(1024));
    let output = AudioRingBuffer::new(1024);
    let dummy_input = AudioRingBuffer::new(1);

    let mut mixer = Mixer::new("bench_mixer");
    mixer.connect_input("input_a", 1.0, input_a.clone());
    mixer.connect_input("input_b", 1.0, input_b.clone());

    let frame = PcmFrame {
        utc_ns: 0,
        samples: vec![1; 96],
        sample_rate: 48_000,
        channels: 2,
    };

    let iterations = 10_000;
    let start = Instant::now();
    for _ in 0..iterations {
        input_a.push(frame.clone());
        input_b.push(frame.clone());
        mixer.process(&dummy_input, &output).expect("mixer process");
        let _ = output.pop_for_reader("bench_output");
    }
    let elapsed = start.elapsed();

    println!(
        "Mixer benchmark: {} iterations in {:.2?} ({:.0} it/s)",
        iterations,
        elapsed,
        iterations as f64 / elapsed.as_secs_f64()
    );
}
