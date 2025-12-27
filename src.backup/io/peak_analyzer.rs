use crate::ring::{RingRead, RingReader};

use serde::{Deserialize, Serialize};

use std::time::Instant;

/// Ergebnis wie im Python-Receiver
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeakEvent {
    pub seq: u64,
    pub utc_ns: u64,
    pub peak_l: f32,
    pub peak_r: f32,
    pub silence: bool,
    pub latency_ms: f32,
}

/// Callback-Typ: Empfänger entscheidet (HTTP, Influx, Log, egal)
pub type PeakHandler = Box<dyn Fn(&PeakEvent) + Send>;

pub struct PeakAnalyzer {
    reader: RingReader,
    handler: PeakHandler,

    // Throttle
    last_emit: Instant,
    min_interval_ms: u64,
}

impl PeakAnalyzer {
    pub fn new(
        reader: RingReader,
        handler: PeakHandler,
        min_interval_ms: u64, // z.B. 100
    ) -> Self {
        Self {
            reader,
            handler,
            last_emit: Instant::now(),
            min_interval_ms,
        }
    }

    /// Poll-Schleife (blocking, wie deine anderen IO-Module)
    pub fn run(&mut self) {
        loop {
            match self.reader.poll() {
                RingRead::Chunk(slot) => {
                    let now = Instant::now();
                    if now.duration_since(self.last_emit).as_millis() < self.min_interval_ms as u128
                    {
                        continue;
                    }
                    self.last_emit = now;

                    let (peak_l, peak_r, silence) = scan_peaks(&slot.pcm);

                    let latency_ms = (std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos() as i128
                        - slot.utc_ns as i128) as f32
                        / 1e6;

                    let evt = PeakEvent {
                        seq: slot.seq,
                        utc_ns: slot.utc_ns,
                        peak_l,
                        peak_r,
                        silence,
                        latency_ms,
                    };

                    (self.handler)(&evt);
                }

                RingRead::Gap { missed } => {
                    eprintln!("[peak_analyzer] GAP: missed {}", missed);
                }

                RingRead::Empty => {
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
            }
        }
    }
}

/// Schneller Single-Pass Peak-Scan (s16le interleaved)
fn scan_peaks(pcm: &[i16]) -> (f32, f32, bool) {
    let mut peak_l = 0.0f32;
    let mut peak_r = 0.0f32;

    let mut it = pcm.iter();
    while let (Some(l), Some(r)) = (it.next(), it.next()) {
        let fl = (*l as f32).abs() / 32768.0;
        let fr = (*r as f32).abs() / 32768.0;
        if fl > peak_l {
            peak_l = fl;
        }
        if fr > peak_r {
            peak_r = fr;
        }
    }

    let silence = peak_l < 0.01 && peak_r < 0.01;
    (peak_l, peak_r, silence)
}
