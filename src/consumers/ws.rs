use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use anyhow::Result;
use crossbeam_channel::{Sender, Receiver, unbounded};

use crate::core::{Consumer, ConsumerStatus, PcmFrame, ringbuffer::AudioRingBuffer};

pub struct WsConsumer {
    name: String,
    sender: Option<Sender<PcmFrame>>,
    connected: Arc<AtomicBool>,
    input_buffer: Option<Arc<AudioRingBuffer>>,
    frames_processed: Arc<AtomicU64>,
    bytes_written: Arc<AtomicU64>,
    errors: Arc<AtomicU64>,
    reader_id: String,
    thread_handle: Option<std::thread::JoinHandle<()>>,
    // NEU: Echo-Konfiguration
    echo_mode: bool,
    echo_target_interval_ms: u64,
}

impl WsConsumer {
    pub fn new(name: &str) -> (Self, Receiver<PcmFrame>) {
        let (sender, receiver) = unbounded();
        (
            Self {
                name: name.to_string(),
                sender: Some(sender),
                connected: Arc::new(AtomicBool::new(false)),
                input_buffer: None,
                frames_processed: Arc::new(AtomicU64::new(0)),
                bytes_written: Arc::new(AtomicU64::new(0)),
                errors: Arc::new(AtomicU64::new(0)),
                reader_id: format!("consumer:{}", name),
                thread_handle: None,
                // WICHTIG: 21ms für 1024 Samples bei 48kHz (~47 FPS)
                echo_mode: false,
                echo_target_interval_ms: 21,
            },
            receiver
        )
    }
    
    // NEUE METHODE: Für Echo konfigurieren
    pub fn set_echo_mode(&mut self, enabled: bool) {
        self.echo_mode = enabled;
        if enabled {
            log::info!("WsConsumer '{}' configured for echo mode ({}ms interval = ~{} FPS)", 
                self.name, self.echo_target_interval_ms, 
                1000 / self.echo_target_interval_ms);
        }
    }
}

impl Consumer for WsConsumer {
    fn name(&self) -> &str {
        &self.name
    }

    fn start(&mut self) -> Result<()> {
        if self.connected.load(Ordering::Relaxed) {
            return Ok(());
        }

        self.connected.store(true, Ordering::SeqCst);

        // Start processing thread
        let connected = self.connected.clone();
        let input_buffer = self.input_buffer.clone();
        let sender = self.sender.clone();
        let reader_id = self.reader_id.clone();
        let frames_processed = self.frames_processed.clone();
        let bytes_written = self.bytes_written.clone();
        let errors = self.errors.clone();
        let name = self.name.clone();
        
        // Echo-spezifische Parameter
        let echo_mode = self.echo_mode;
        let echo_interval = Duration::from_millis(self.echo_target_interval_ms);

        let handle = std::thread::spawn(move || {
            log::info!("WsConsumer '{}' thread started (echo_mode: {})", name, echo_mode);
            let mut last_stats = Instant::now();
            let mut last_echo_sent = Instant::now();
            let mut echo_frame_buffer = Vec::new();
            
            while connected.load(Ordering::Relaxed) {
                if let Some(buffer) = &input_buffer {
                    if last_stats.elapsed() >= Duration::from_secs(5) {
                        let available = buffer.available_for_reader(&reader_id);
                        let stats = buffer.stats();
                        log::info!(
                            "WsConsumer '{}' stats: available={}, buffer_frames={}, dropped={}, processed={}, errors={}, echo_buffer={}",
                            name,
                            available,
                            stats.current_frames,
                            stats.dropped_frames,
                            frames_processed.load(Ordering::Relaxed),
                            errors.load(Ordering::Relaxed),
                            echo_frame_buffer.len()
                        );
                        last_stats = Instant::now();
                    }

                    if let Some(frame) = buffer.pop_for_reader(&reader_id) {
                        if echo_mode {
                            // ECHO MODUS: Frame BEHALTEN in Originalgröße (1024 Samples)
                            // Keine Reduzierung auf 512 Samples!
                            echo_frame_buffer.push(frame);
                            
                            // Buffer auf 20 Frames beschränken (~420ms bei 21ms/Frame)
                            if echo_frame_buffer.len() > 20 {
                                // Den ältesten Frame entfernen
                                echo_frame_buffer.remove(0);
                                errors.fetch_add(1, Ordering::Relaxed);
                                log::trace!("Echo buffer overflow, dropped oldest frame");
                            }
                            
                            // Prüfen ob wir senden sollten (alle 21ms = ~47 FPS)
                            let now = Instant::now();
                            if now.duration_since(last_echo_sent) >= echo_interval && !echo_frame_buffer.is_empty() {
                                last_echo_sent = now;
                                
                                // Den ältesten Frame nehmen und senden
                                let frame_to_send = echo_frame_buffer.remove(0);
                                let frame_size = frame_to_send.samples.len() * 2;
                                
                                if let Some(sender) = &sender {
                                    if sender.send(frame_to_send).is_ok() {
                                        frames_processed.fetch_add(1, Ordering::Relaxed);
                                        bytes_written.fetch_add(frame_size as u64, Ordering::Relaxed);
                                        log::trace!("WsConsumer '{}' sent echo frame ({} samples)", 
                                            name, frame_size / 2);
                                    } else {
                                        errors.fetch_add(1, Ordering::Relaxed);
                                        log::warn!("WsConsumer '{}' failed to send echo frame", name);
                                    }
                                }
                            }
                        } else {
                            // NORMAL MODUS: Direkt senden
                            let frame_size = frame.samples.len() * 2;
                            
                            if let Some(sender) = &sender {
                                if sender.send(frame).is_ok() {
                                    frames_processed.fetch_add(1, Ordering::Relaxed);
                                    bytes_written.fetch_add(frame_size as u64, Ordering::Relaxed);
                                    log::trace!("WsConsumer '{}' sent frame", name);
                                } else {
                                    errors.fetch_add(1, Ordering::Relaxed);
                                    log::warn!("WsConsumer '{}' failed to send frame", name);
                                }
                            }
                        }
                    } else {
                        // Kürzere Sleep-Zeit für Echo-Modus
                        let sleep_ms = if echo_mode { 2 } else { 10 };
                        std::thread::sleep(std::time::Duration::from_millis(sleep_ms));
                    }
                } else {
                    if last_stats.elapsed() >= Duration::from_secs(5) {
                        log::info!(
                            "WsConsumer '{}' waiting for input buffer",
                            name
                        );
                        last_stats = Instant::now();
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }
            
            log::info!("WsConsumer '{}' thread stopped", name);
        });

        self.thread_handle = Some(handle);
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        self.connected.store(false, Ordering::SeqCst);

        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }

        Ok(())
    }

    fn status(&self) -> ConsumerStatus {
        ConsumerStatus {
            running: self.connected.load(Ordering::Relaxed),
            connected: self.input_buffer.is_some(),
            frames_processed: self.frames_processed.load(Ordering::Relaxed),
            bytes_written: self.bytes_written.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
        }
    }

    fn attach_input_buffer(&mut self, buffer: Arc<AudioRingBuffer>) {
        self.input_buffer = Some(buffer);
        log::info!("WsConsumer '{}' attached to buffer", self.name);
    }
}
