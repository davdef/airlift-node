use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};

use anyhow::Result;

use crate::core::lock::lock_mutex;
use crate::core::{timestamp, AudioRingBuffer, PcmFrame, Producer, ProducerStatus};
use crate::impl_connectable_producer;

#[derive(Debug)]
struct WsState {
    name: String,
    ring: Mutex<Option<Arc<AudioRingBuffer>>>,
    running: AtomicBool,
    samples_processed: AtomicU64,
    errors: AtomicU64,
    last_log_ns: AtomicU64,
}

#[derive(Clone)]
pub struct WsHandle {
    state: Arc<WsState>,
}

impl WsHandle {
    pub fn push_frame(&self, frame: PcmFrame) -> Result<()> {
        let ring = lock_mutex(&self.state.ring, "ws.handle.push_frame");
        if let Some(rb) = ring.as_ref() {
            let samples_len = frame.samples.len() as u64;
            rb.push(frame);
            self.state
                .samples_processed
                .fetch_add(samples_len, Ordering::Relaxed);
            let now = timestamp::utc_ns_now();
            let last_log = self.state.last_log_ns.load(Ordering::Relaxed);
            if last_log == 0 || now.saturating_sub(last_log) >= 5_000_000_000 {
                self.state.last_log_ns.store(now, Ordering::Relaxed);
                let stats = rb.stats();
                log::info!(
                    "WsProducer '{}' stats: buffer_frames={}, dropped_frames={}, samples_processed={}, errors={}",
                    self.state.name,
                    stats.current_frames,
                    stats.dropped_frames,
                    self.state.samples_processed.load(Ordering::Relaxed),
                    self.state.errors.load(Ordering::Relaxed)
                );
            }
            Ok(())
        } else {
            self.state.errors.fetch_add(1, Ordering::Relaxed);
            anyhow::bail!("ws buffer not attached");
        }
    }
}

pub struct WsProducer {
    name: String,
    state: Arc<WsState>,
}

impl WsProducer {
    pub fn new(name: &str) -> (Self, WsHandle) {
        let state = Arc::new(WsState {
            name: name.to_string(),
            ring: Mutex::new(None),
            running: AtomicBool::new(false),
            samples_processed: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            last_log_ns: AtomicU64::new(0),
        });
        (
            Self {
                name: name.to_string(),
                state: state.clone(),
            },
            WsHandle { state },
        )
    }
}

impl Producer for WsProducer {
    fn name(&self) -> &str {
        &self.name
    }

    fn start(&mut self) -> Result<()> {
        self.state.running.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        self.state.running.store(false, Ordering::SeqCst);
        Ok(())
    }

    fn status(&self) -> ProducerStatus {
        let ring = lock_mutex(&self.state.ring, "ws.producer.status");
        ProducerStatus {
            running: self.state.running.load(Ordering::Relaxed),
            connected: ring.is_some(),
            samples_processed: self.state.samples_processed.load(Ordering::Relaxed),
            errors: self.state.errors.load(Ordering::Relaxed),
            buffer_stats: ring.as_ref().map(|r| r.stats()),
        }
    }

    fn attach_ring_buffer(&mut self, buffer: Arc<AudioRingBuffer>) {
        let mut ring = lock_mutex(&self.state.ring, "ws.producer.attach_ring_buffer");
        *ring = Some(buffer);
    }
}

impl_connectable_producer!(WsProducer);
