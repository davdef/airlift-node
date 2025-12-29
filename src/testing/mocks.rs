use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use anyhow::Result;

use crate::core::consumer::{Consumer, ConsumerStatus};
use crate::core::{Producer, ProducerStatus};
use crate::core::ringbuffer::{AudioRingBuffer, PcmFrame};

pub struct MockProducer {
    name: String,
    running: Arc<AtomicBool>,
    ring_buffer: Option<Arc<AudioRingBuffer>>,
    frames: Vec<PcmFrame>,
    thread_handle: Option<std::thread::JoinHandle<()>>,
    samples_processed: Arc<AtomicU64>,
    errors: Arc<AtomicU64>,
}

impl MockProducer {
    pub fn new(name: &str, frames: Vec<PcmFrame>) -> Self {
        Self {
            name: name.to_string(),
            running: Arc::new(AtomicBool::new(false)),
            ring_buffer: None,
            frames,
            thread_handle: None,
            samples_processed: Arc::new(AtomicU64::new(0)),
            errors: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn samples_processed(&self) -> u64 {
        self.samples_processed.load(Ordering::Relaxed)
    }
}

impl Producer for MockProducer {
    fn name(&self) -> &str {
        &self.name
    }

    fn start(&mut self) -> Result<()> {
        if self.running.load(Ordering::Relaxed) {
            return Ok(());
        }

        let buffer = self
            .ring_buffer
            .clone()
            .ok_or_else(|| anyhow::anyhow!("MockProducer '{}' missing ring buffer", self.name))?;

        self.running.store(true, Ordering::SeqCst);

        let running = self.running.clone();
        let frames = std::mem::take(&mut self.frames);
        let samples_processed = self.samples_processed.clone();
        let errors = self.errors.clone();

        let handle = std::thread::spawn(move || {
            for frame in frames {
                if !running.load(Ordering::Relaxed) {
                    break;
                }
                samples_processed.fetch_add(frame.samples.len() as u64, Ordering::Relaxed);
                buffer.push(frame);
            }
            running.store(false, Ordering::SeqCst);
            errors.fetch_add(0, Ordering::Relaxed);
        });

        self.thread_handle = Some(handle);
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        self.running.store(false, Ordering::SeqCst);

        if let Some(handle) = self.thread_handle.take() {
            if handle.join().is_err() {
                self.errors.fetch_add(1, Ordering::Relaxed);
            }
        }

        Ok(())
    }

    fn status(&self) -> ProducerStatus {
        ProducerStatus {
            running: self.running.load(Ordering::Relaxed),
            connected: self.ring_buffer.is_some(),
            samples_processed: self.samples_processed.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
            buffer_stats: self.ring_buffer.as_ref().map(|buffer| buffer.stats()),
        }
    }

    fn attach_ring_buffer(&mut self, buffer: Arc<AudioRingBuffer>) {
        self.ring_buffer = Some(buffer);
    }
}

pub struct MockConsumer {
    name: String,
    running: Arc<AtomicBool>,
    input_buffer: Option<Arc<AudioRingBuffer>>,
    reader_id: String,
    thread_handle: Option<std::thread::JoinHandle<()>>,
    frames_processed: Arc<AtomicU64>,
    bytes_written: Arc<AtomicU64>,
    errors: Arc<AtomicU64>,
    received_frames: Arc<Mutex<Vec<PcmFrame>>>,
}

impl MockConsumer {
    pub fn new(name: &str) -> Self {
        Self::new_with_shared(name).0
    }

    pub fn new_with_shared(name: &str) -> (Self, Arc<Mutex<Vec<PcmFrame>>>) {
        let received_frames = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                name: name.to_string(),
                running: Arc::new(AtomicBool::new(false)),
                input_buffer: None,
                reader_id: format!("mock-consumer:{}", name),
                thread_handle: None,
                frames_processed: Arc::new(AtomicU64::new(0)),
                bytes_written: Arc::new(AtomicU64::new(0)),
                errors: Arc::new(AtomicU64::new(0)),
                received_frames: received_frames.clone(),
            },
            received_frames,
        )
    }

    pub fn received_frames(&self) -> Arc<Mutex<Vec<PcmFrame>>> {
        self.received_frames.clone()
    }
}

impl Consumer for MockConsumer {
    fn name(&self) -> &str {
        &self.name
    }

    fn start(&mut self) -> Result<()> {
        if self.running.load(Ordering::Relaxed) {
            return Ok(());
        }

        let buffer = self
            .input_buffer
            .clone()
            .ok_or_else(|| anyhow::anyhow!("MockConsumer '{}' missing input buffer", self.name))?;

        self.running.store(true, Ordering::SeqCst);

        let running = self.running.clone();
        let reader_id = self.reader_id.clone();
        let frames_processed = self.frames_processed.clone();
        let bytes_written = self.bytes_written.clone();
        let errors = self.errors.clone();
        let received_frames = self.received_frames.clone();

        let handle = std::thread::spawn(move || {
            while running.load(Ordering::Relaxed) {
                if let Some(frame) = buffer.pop_for_reader(&reader_id) {
                    bytes_written.fetch_add((frame.samples.len() * 2) as u64, Ordering::Relaxed);
                    frames_processed.fetch_add(1, Ordering::Relaxed);
                    received_frames.lock().expect("lock received_frames").push(frame);
                } else {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
            }
            errors.fetch_add(0, Ordering::Relaxed);
        });

        self.thread_handle = Some(handle);
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        self.running.store(false, Ordering::SeqCst);

        if let Some(handle) = self.thread_handle.take() {
            if handle.join().is_err() {
                self.errors.fetch_add(1, Ordering::Relaxed);
            }
        }

        Ok(())
    }

    fn status(&self) -> ConsumerStatus {
        ConsumerStatus {
            running: self.running.load(Ordering::Relaxed),
            connected: self.input_buffer.is_some(),
            frames_processed: self.frames_processed.load(Ordering::Relaxed),
            bytes_written: self.bytes_written.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
        }
    }

    fn attach_input_buffer(&mut self, buffer: Arc<AudioRingBuffer>) {
        self.input_buffer = Some(buffer);
    }
}
