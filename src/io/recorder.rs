use crate::ring::{RingReader, RingRead};
use hound::{WavSpec, WavWriter};
use std::fs::create_dir_all;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const RATE: u32 = 48_000;
const CHANNELS: u16 = 2;
const BITS: u16 = 16;

pub struct RecorderConfig {
    pub base_dir: PathBuf, // z.B. /srv/rfm/aircheck/chunks
}

pub fn run_recorder(mut r: RingReader, cfg: RecorderConfig) -> anyhow::Result<()> {
    create_dir_all(&cfg.base_dir)?;

    let spec = WavSpec {
        channels: CHANNELS,
        sample_rate: RATE,
        bits_per_sample: BITS,
        sample_format: hound::SampleFormat::Int,
    };

    let mut written: u64 = 0;

    // --- Stats ---
    let mut gaps: u64 = 0;
    let mut missed_total: u64 = 0;
    let mut last_log = Instant::now();

    loop {
        match r.poll() {
            RingRead::Chunk(slot) => {
                // Dateiname: utc_ns.wav
                let mut path = cfg.base_dir.clone();
                path.push(format!("{}.wav", slot.utc_ns));

                let mut w = WavWriter::create(&path, spec)?;
                for s in slot.pcm.iter() {
                    w.write_sample(*s)?;
                }
                w.finalize()?;

                written += 1;
                if written % 10 == 0 {
                    println!("[recorder] wrote {} chunks", written);
                }
            }

            RingRead::Gap { missed } => {
                gaps += 1;
                missed_total += missed;
            }

            RingRead::Empty => {
                std::thread::sleep(Duration::from_millis(5));
            }
        }

        // --- Periodisches Logging ---
        if last_log.elapsed() >= Duration::from_secs(5) {
            let fill = r.fill();

            eprintln!(
                "[recorder] fill={} slots | GAPs={} missed={}",
                fill, gaps, missed_total
            );

            gaps = 0;
            missed_total = 0;
            last_log = Instant::now();
        }
    }
}
