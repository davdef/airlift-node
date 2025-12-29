use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::fs::File;
use std::io::Read;
use anyhow::{Result, anyhow};

use crate::core::{Producer, ProducerStatus, AudioRingBuffer};
use crate::producers::wait::StopWait;

pub struct FileProducer {
    name: String,
    running: Arc<AtomicBool>,
    samples_processed: Arc<AtomicU64>,
    config: crate::config::ProducerConfig,
    thread_handle: Option<std::thread::JoinHandle<()>>,
    ring_buffer: Option<Arc<AudioRingBuffer>>,
    stop_wait: Arc<StopWait>,
}

impl FileProducer {
    pub fn new(name: &str, config: &crate::config::ProducerConfig) -> Self {
        Self {
            name: name.to_string(),
            running: Arc::new(AtomicBool::new(false)),
            samples_processed: Arc::new(AtomicU64::new(0)),
            config: config.clone(),
            thread_handle: None,
            ring_buffer: None,
            stop_wait: Arc::new(StopWait::new()),
        }
    }
    
    fn utc_ns_now() -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64
    }
}

impl Producer for FileProducer {
    fn name(&self) -> &str {
        &self.name
    }
    
    fn start(&mut self) -> Result<()> {
        if self.running.load(Ordering::Relaxed) {
            return Ok(());
        }
        
        log::info!("FileProducer '{}': Starting (path: {}, loop: {})", 
            self.name, 
            self.config.path.as_deref().unwrap_or("none"),
            self.config.loop_audio.unwrap_or(false)
        );
        
        let path = self.config.path.clone()
            .ok_or_else(|| anyhow!("No file path specified"))?;
            
        let sample_rate = self.config.sample_rate.unwrap_or(48000);
        let channels = self.config.channels.unwrap_or(2);
        let loop_audio = self.config.loop_audio.unwrap_or(false);
        
        self.running.store(true, Ordering::SeqCst);
        
        let running = self.running.clone();
        let name = self.name.clone();
        let samples_processed = self.samples_processed.clone();
        let ring_buffer = self.ring_buffer.clone();
        let stop_wait = self.stop_wait.clone();
        
        let handle = std::thread::spawn(move || {
            log::info!("FileProducer '{}': Playing {}", name, path);
            
            let mut iteration = 0;
            while running.load(Ordering::Relaxed) {
                iteration += 1;
                
                match File::open(&path) {
                    Ok(mut file) => {
                        // Einfache Simulation: Erzeuge Test-Daten
                        let samples_per_frame = (sample_rate as usize / 10) * channels as usize; // 100ms
                        let mut chunk = vec![0i16; samples_per_frame];
                        
                        // Fülle mit Test-Daten (Sinus-ähnlich)
                        for (i, sample) in chunk.iter_mut().enumerate() {
                            let t = i as f32 / sample_rate as f32;
                            *sample = (t.sin() * 10000.0) as i16;
                        }
                        
                        // In RingBuffer speichern
                        if let Some(rb) = &ring_buffer {
                            let frame = crate::core::ringbuffer::PcmFrame {
                                utc_ns: Self::utc_ns_now(),
                                samples: chunk.to_vec(),
                                sample_rate,
                                channels,
                            };
                            log::debug!("FileProducer '{}': Schreibe Frame {} ({} samples) in Buffer", 
                                name, iteration, chunk.len());
                            rb.push(frame);
                            samples_processed.fetch_add(chunk.len() as u64, Ordering::Relaxed);
                        } else {
                            log::warn!("FileProducer '{}': KEIN Buffer attached!", name);
                        }
                        
                        log::debug!("FileProducer '{}': Generated frame", name);
                    }
                    Err(e) => {
                        log::error!("FileProducer '{}': Failed to open {}: {}", name, path, e);
                        break;
                    }
                }
                
                stop_wait.wait_timeout(std::time::Duration::from_millis(100)); // 10 FPS
                
                if !loop_audio {
                    break;
                }
            }
            
            log::info!("FileProducer '{}': Thread stopped", name);
        });
        
        self.thread_handle = Some(handle);
        Ok(())
    }
    
    fn stop(&mut self) -> Result<()> {
        log::info!("FileProducer '{}': Stopping...", self.name);
        self.running.store(false, Ordering::SeqCst);
        self.stop_wait.notify_all();
        
        if let Some(handle) = self.thread_handle.take() {
            if let Err(e) = handle.join() {
                log::error!("Failed to join producer thread: {:?}", e);
            }
        }
        
        Ok(())
    }
    
    fn status(&self) -> ProducerStatus {
        ProducerStatus {
            running: self.running.load(Ordering::Relaxed),
            connected: self.ring_buffer.is_some(),
            samples_processed: self.samples_processed.load(Ordering::Relaxed),
            errors: 0,
            buffer_stats: self.ring_buffer.as_ref().map(|b| b.stats()),
        }
    }
    
    fn attach_ring_buffer(&mut self, buffer: Arc<AudioRingBuffer>) {
        log::debug!("FileProducer '{}': attach_ring_buffer mit addr: {:?}", 
            self.name, Arc::as_ptr(&buffer));
        self.ring_buffer = Some(buffer);
    }
}
