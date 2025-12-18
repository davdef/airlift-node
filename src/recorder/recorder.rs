// src/recorder/recorder.rs - MIT CONTINUITY CHECKS
use std::time::{Instant, Duration};

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
    let mut last_continuity_check = Instant::now();
    
    // Kontinuitäts-Intervall (alle 100ms)
    const CONTINUITY_INTERVAL: Duration = Duration::from_millis(100);

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
                    
                    // Nach Stundenwechsel sofort Kontinuität prüfen
                    last_continuity_check = Instant::now();
                }

                // Chunk an alle Sinks
                for s in sinks.iter_mut() {
                    s.on_chunk(&slot)?;
                }
                
                // Nach Audio auch Kontinuität zurücksetzen
                last_continuity_check = Instant::now();
            }

            RingRead::Gap { .. } => {
                // Lücke - trotzdem Kontinuität wahren
            }

            RingRead::Empty => {
                std::thread::sleep(cfg.idle_sleep);
            }
        }

        // REGELMÄSSIG: Kontinuität wahren (auch wenn keine Audio kommt)
        if last_continuity_check.elapsed() >= CONTINUITY_INTERVAL {
            for s in sinks.iter_mut() {
                if let Err(e) = s.maintain_continuity() {
                    eprintln!("[recorder] continuity error: {}", e);
                }
            }
            last_continuity_check = Instant::now();
        }

        // Retention periodisch ausführen (z.B. 1×/h)
        if last_retention.elapsed() >= cfg.retention_interval {
            if let Some(h) = current_hour {
                for r in retentions.iter_mut() {
                    if let Err(e) = r.run(h) {
                        eprintln!("[recorder] retention error: {}", e);
                    }
                }
            }
            last_retention = Instant::now();
        }
    }
}
