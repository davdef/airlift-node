use crate::impl_connectable_producer;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;

use crate::core::{AudioRingBuffer, PcmFrame, Producer, ProducerStatus};
use crate::producers::wait::StopWait;

// Timing constant for sine wave generation loop.
const SINE_POLL_INTERVAL_MS: u64 = 10; // 100 Hz

pub struct SineProducer {
    name: String,
    running: Arc<AtomicBool>,
    samples_processed: Arc<AtomicU64>,
    ring: Option<Arc<AudioRingBuffer>>,
    freq: f32,
    sample_rate: u32,
    stop_wait: Arc<StopWait>,
}

impl SineProducer {
    pub fn new(name: &str, freq: f32, sample_rate: u32) -> Self {
        Self {
            name: name.to_string(),
            running: Arc::new(AtomicBool::new(false)),
            samples_processed: Arc::new(AtomicU64::new(0)),
            ring: None,
            freq,
            sample_rate,
            stop_wait: Arc::new(StopWait::new()),
        }
    }
}

impl Producer for SineProducer {
    fn name(&self) -> &str {
        &self.name
    }

    fn start(&mut self) -> anyhow::Result<()> {
        if self.running.load(Ordering::Relaxed) {
            return Ok(());
        }
        let ring = self.ring.clone();
        let running = self.running.clone();
        let samples_processed = self.samples_processed.clone();

        let freq = self.freq;
        let rate = self.sample_rate;

        let stop_wait = self.stop_wait.clone();

        thread::spawn(move || {
            let mut phase: f32 = 0.0;
            let step = 2.0 * std::f32::consts::PI * freq / rate as f32;

            while running.load(Ordering::Relaxed) {
                let mut samples = Vec::with_capacity(480 * 2);
                for _ in 0..480 {
                    let v = (phase.sin() * 0.2 * i16::MAX as f32) as i16;
                    samples.push(v);
                    samples.push(v);
                    phase += step;
                }

                samples_processed.fetch_add(samples.len() as u64, Ordering::Relaxed);

                if let Some(rb) = &ring {
                    rb.push(PcmFrame {
                        utc_ns: crate::core::timestamp::utc_ns_now(),
                        samples,
                        sample_rate: rate,
                        channels: 2,
                    });
                }

                stop_wait.wait_timeout(Duration::from_millis(SINE_POLL_INTERVAL_MS));
            }
        });

        self.running.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        self.running.store(false, Ordering::SeqCst);
        self.stop_wait.notify_all();
        Ok(())
    }

    fn status(&self) -> ProducerStatus {
        ProducerStatus {
            running: self.running.load(Ordering::Relaxed),
            connected: true,
            samples_processed: self.samples_processed.load(Ordering::Relaxed),
            errors: 0,
            buffer_stats: self.ring.as_ref().map(|r| r.stats()),
        }
    }

    fn attach_ring_buffer(&mut self, buffer: Arc<AudioRingBuffer>) {
        self.ring = Some(buffer);
    }
}

impl_connectable_producer!(SineProducer);
