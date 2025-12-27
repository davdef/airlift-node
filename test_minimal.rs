use std::sync::Arc;
use std::time::Duration;

fn main() -> anyhow::Result<()> {
    // Einfacher Test ohne komplexe Architektur
    
    // 1. Erstelle einen Buffer
    let buffer = Arc::new(airlift_node::core::ringbuffer::AudioRingBuffer::new(100));
    
    // 2. Erstelle einen einfachen Producer, der Test-Daten schreibt
    std::thread::spawn({
        let buffer = buffer.clone();
        move || {
            let mut frame_count = 0;
            loop {
                let frame = airlift_node::core::ringbuffer::PcmFrame {
                    utc_ns: 0,
                    samples: vec![1000, -1000, 500, -500], // 2 Channels, 2 Samples pro Channel
                    sample_rate: 48000,
                    channels: 2,
                };
                
                if let Ok(_) = buffer.try_push(frame) {
                    frame_count += 1;
                    println!("Producer: Wrote frame {}", frame_count);
                }
                
                std::thread::sleep(Duration::from_millis(100));
                
                if frame_count >= 10 {
                    break;
                }
            }
        }
    });
    
    // 3. Erstelle einen Consumer, der liest
    let buffer_clone = buffer.clone();
    std::thread::spawn(move || {
        let mut frame_count = 0;
        loop {
            if let Some(frame) = buffer_clone.pop() {
                frame_count += 1;
                println!("Consumer: Read frame {} with {} samples", frame_count, frame.samples.len());
            }
            
            std::thread::sleep(Duration::from_millis(50));
            
            if frame_count >= 10 {
                break;
            }
        }
    });
    
    // Warte auf Fertigstellung
    std::thread::sleep(Duration::from_secs(2));
    
    println!("Test abgeschlossen. Buffer Länge am Ende: {}", buffer.len());
    
    Ok(())
}
