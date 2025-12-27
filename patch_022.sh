# 1. Consumer-Trait definieren
cat > src/core/consumer.rs << 'EOF'
use std::sync::Arc;
use anyhow::Result;
use crate::core::ringbuffer::AudioRingBuffer;

pub trait Consumer: Send + Sync {
    fn name(&self) -> &str;
    fn start(&mut self) -> Result<()>;
    fn stop(&mut self) -> Result<()>;
    fn status(&self) -> ConsumerStatus;
    fn attach_input_buffer(&mut self, buffer: Arc<AudioRingBuffer>);
}

#[derive(Debug, Clone)]
pub struct ConsumerStatus {
    pub running: bool,
    pub connected: bool,
    pub frames_processed: u64,
    pub bytes_sent: u64,
    pub errors: u64,
}

// Beispiel-Consumer: File Writer
pub mod file_writer {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    
    pub struct FileConsumer {
        name: String,
        running: std::sync::Arc<std::sync::atomic::AtomicBool>,
        input_buffer: Option<Arc<AudioRingBuffer>>,
        output_path: String,
        thread_handle: Option<std::thread::JoinHandle<()>>,
        frames_processed: std::sync::Arc<std::sync::atomic::AtomicU64>,
    }
    
    impl FileConsumer {
        pub fn new(name: &str, output_path: &str) -> Self {
            Self {
                name: name.to_string(),
                running: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                input_buffer: None,
                output_path: output_path.to_string(),
                thread_handle: None,
                frames_processed: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
            }
        }
    }
    
    impl Consumer for FileConsumer {
        fn name(&self) -> &str {
            &self.name
        }
        
        fn start(&mut self) -> Result<()> {
            log::info!("FileConsumer '{}' starting to write to {}", self.name, self.output_path);
            self.running.store(true, std::sync::atomic::Ordering::SeqCst);
            
            let running = self.running.clone();
            let input_buffer = self.input_buffer.clone();
            let output_path = self.output_path.clone();
            let frames_processed = self.frames_processed.clone();
            
            let handle = std::thread::spawn(move || {
                // Demo: Simuliere File Writing
                while running.load(std::sync::atomic::Ordering::Relaxed) {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    
                    if let Some(buffer) = &input_buffer {
                        if let Some(frame) = buffer.pop() {
                            frames_processed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            log::debug!("FileConsumer: Would write frame to {}", output_path);
                        }
                    }
                }
                log::info!("FileConsumer stopped");
            });
            
            self.thread_handle = Some(handle);
            Ok(())
        }
        
        fn stop(&mut self) -> Result<()> {
            self.running.store(false, std::sync::atomic::Ordering::SeqCst);
            if let Some(handle) = self.thread_handle.take() {
                handle.join().map_err(|e| anyhow::anyhow!("Join error: {:?}", e))?;
            }
            Ok(())
        }
        
        fn status(&self) -> ConsumerStatus {
            ConsumerStatus {
                running: self.running.load(std::sync::atomic::Ordering::Relaxed),
                connected: self.input_buffer.is_some(),
                frames_processed: self.frames_processed.load(std::sync::atomic::Ordering::Relaxed),
                bytes_sent: 0,
                errors: 0,
            }
        }
        
        fn attach_input_buffer(&mut self, buffer: Arc<AudioRingBuffer>) {
            self.input_buffer = Some(buffer);
        }
    }
}
EOF

# 2. Flow für Consumer erweitern
echo "Für Consumer und parallele/serielle Verarbeitung brauchen wir:"
echo ""
echo "A) Graph-Struktur für Prozessoren:"
echo "   processor1 → processor2 (seriell)"
echo "   processor1 → processor3 (parallel)"
echo ""
echo "B) Consumer an Outputs anhängen"
echo ""
echo "Sollen wir:"
echo "1. Consumer zuerst implementieren (einfacher)"
echo "2. Graph-Struktur für Prozessoren (komplexer)"
echo "3. Beides zusammen (ambitioniert)"
