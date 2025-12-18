// src/recorder/recorder.rs

use std::time::Instant;

use crate::ring::{RingRead, RingReader};
use super::{AudioSink, RetentionPolicy, RecorderConfig};

pub fn run_recorder(
    mut reader: RingReader,
    cfg: RecorderConfig,
    mut sinks: Vec<Box<dyn AudioSink>>,
    mut retentions: Vec<Box<dyn RetentionPolicy>>,
) -> anyhow::Result<()> {

    let mut current_hour: Option<u64> = None;
    let mut last_retention = Instant::now();

    loop {
        match reader.poll() {
            RingRead::Chunk(slot) => {
                // UTC-Nanosekunden → Stundenindex
                let hour = slot.utc_ns / 1_000_000_000 / 3600;

                // Stundenwechsel
                if current_hour != Some(hour) {
                    for s in sinks.iter_mut() {
                        s.on_hour_change(hour)?;
                    }
                    current_hour = Some(hour);
                }

                // Chunk an alle Sinks
                for s in sinks.iter_mut() {
                    s.on_chunk(&slot)?;
                }
            }

            RingRead::Gap { .. } => {
                // absichtlich leer
            }

            RingRead::Empty => {
                std::thread::sleep(cfg.idle_sleep);
            }
        }

        // Retention periodisch ausführen (z.B. 1×/h)
        if last_retention.elapsed() >= cfg.retention_interval {
            if let Some(h) = current_hour {
                for r in retentions.iter_mut() {
                    r.run(h)?;
                }
            }
            last_retention = Instant::now();
        }
    }
}
