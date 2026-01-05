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
            },
            receiver
        )
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

        let handle = std::thread::spawn(move || {
            log::info!("WsConsumer '{}' thread started", name);
            let mut last_stats = Instant::now();
            
            while connected.load(Ordering::Relaxed) {
                if let Some(buffer) = &input_buffer {
                    if last_stats.elapsed() >= Duration::from_secs(5) {
                        let available = buffer.available_for_reader(&reader_id);
                        let stats = buffer.stats();
                        log::info!(
                            "WsConsumer '{}' stats: available_for_reader={}, buffer_frames={}, dropped_frames={}, frames_processed={}, errors={}",
                            name,
                            available,
                            stats.current_frames,
                            stats.dropped_frames,
                            frames_processed.load(Ordering::Relaxed),
                            errors.load(Ordering::Relaxed)
                        );
                        last_stats = Instant::now();
                    }

                    if let Some(frame) = buffer.pop_for_reader(&reader_id) {
                        log::debug!("WsConsumer '{}' got frame with {} samples", name, frame.samples.len());
                        
                        if let Some(sender) = &sender {
                            let frame_size = frame.samples.len() * 2; // i16 = 2 bytes
                            
                            if sender.send(frame).is_ok() {
                                frames_processed.fetch_add(1, Ordering::Relaxed);
                                bytes_written.fetch_add(frame_size as u64, Ordering::Relaxed);
                                log::trace!("WsConsumer '{}' sent frame", name);
                            } else {
                                errors.fetch_add(1, Ordering::Relaxed);
                                log::warn!("WsConsumer '{}' failed to send frame (queue full?)", name);
                            }
                        }
                    } else {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                } else {
                    if last_stats.elapsed() >= Duration::from_secs(5) {
                        log::info!(
                            "WsConsumer '{}' waiting for input buffer (frames_processed={}, errors={})",
                            name,
                            frames_processed.load(Ordering::Relaxed),
                            errors.load(Ordering::Relaxed)
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
