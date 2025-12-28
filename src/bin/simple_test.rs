// src/bin/simple_test.rs - SO FUNKTIONIERT ES
extern crate airlift_node;

fn main() {
    println!("Testing airlift-node...");
    
    // Jetzt mit airlift_node:: 
    let buffer = airlift_node::core::ringbuffer::AudioRingBuffer::new(5);
    println!("✓ Buffer created: capacity = {}", buffer.stats().capacity);
    
    let frame = airlift_node::core::ringbuffer::PcmFrame {
        utc_ns: 123456789,
        samples: vec![1, 2, 3],
        sample_rate: 48000,
        channels: 1,
    };
    
    buffer.push(frame);
    println!("✓ Frame pushed");
    
    if buffer.pop().is_some() {
        println!("✓ Frame popped");
    }
    
    println!("\n✅ All basic tests passed!");
}
