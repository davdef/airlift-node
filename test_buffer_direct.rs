use std::sync::Arc;
use std::thread;
use std::time::Duration;

// Minimaler Buffer-Test ohne externe Abhängigkeiten
fn main() {
    println!("=== Direkter Buffer Test ===");
    
    // Simulierter PcmFrame
    #[derive(Clone, Debug)]
    struct PcmFrame {
        samples: Vec<i16>,
    }
    
    // Vereinfachter RingBuffer
    use std::sync::Mutex;
    use std::collections::VecDeque;
    
    struct SimpleRingBuffer {
        buffer: Mutex<VecDeque<PcmFrame>>,
        capacity: usize,
    }
    
    impl SimpleRingBuffer {
        fn new(capacity: usize) -> Arc<Self> {
            Arc::new(Self {
                buffer: Mutex::new(VecDeque::with_capacity(capacity)),
                capacity,
            })
        }
        
        fn push(&self, frame: PcmFrame) -> Result<(), ()> {
            let mut buffer = self.buffer.lock().unwrap();
            if buffer.len() >= self.capacity {
                return Err(());
            }
            buffer.push_back(frame);
            Ok(())
        }
        
        fn pop(&self) -> Option<PcmFrame> {
            let mut buffer = self.buffer.lock().unwrap();
            buffer.pop_front()
        }
        
        fn len(&self) -> usize {
            let buffer = self.buffer.lock().unwrap();
            buffer.len()
        }
    }
    
    // Test starten
    let buffer = SimpleRingBuffer::new(10);
    
    // Producer Thread
    let producer_buffer = buffer.clone();
    let producer_handle = thread::spawn(move || {
        for i in 0..10 {
            let frame = PcmFrame {
                samples: vec![1000, -1000, 500, -500],
            };
            
            match producer_buffer.push(frame) {
                Ok(()) => println!("Producer: Wrote frame {}", i + 1),
                Err(()) => println!("Producer: Buffer full!"),
            }
            
            thread::sleep(Duration::from_millis(50));
        }
    });
    
    // Consumer Thread
    let consumer_buffer = buffer.clone();
    let consumer_handle = thread::spawn(move || {
        let mut received = 0;
        while received < 10 {
            if let Some(frame) = consumer_buffer.pop() {
                received += 1;
                println!("Consumer: Read frame {} ({} samples)", 
                        received, frame.samples.len());
            }
            thread::sleep(Duration::from_millis(30));
        }
    });
    
    // Warte auf Fertigstellung
    producer_handle.join().unwrap();
    consumer_handle.join().unwrap();
    
    println!("Test abgeschlossen. Final buffer length: {}", buffer.len());
}
