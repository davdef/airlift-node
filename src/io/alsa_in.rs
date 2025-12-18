// src/io/alsa_in.rs
use alsa::pcm::{Access, Format, HwParams, PCM};
use alsa::{Direction, ValueOr};

use std::time::{SystemTime, UNIX_EPOCH};
use std::thread;
use std::time::Duration;

use crate::ring::AudioRing;
use crate::monitoring::Metrics;
use std::sync::Arc;
use std::sync::atomic::Ordering;

const RATE: usize = 48_000;
const CHANNELS: usize = 2;
const TARGET_FRAMES: usize = 4_800; // 100 ms

fn utc_ns_now() -> u64 {
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap();
    d.as_secs() * 1_000_000_000 + d.subsec_nanos() as u64
}

pub fn run_alsa_in(ring: AudioRing, metrics: Arc<Metrics>) -> anyhow::Result<()> {
    // ------------------------------------------------------------
    // PCM öffnen
    // ------------------------------------------------------------
    let pcm = PCM::new("default", Direction::Capture, false)?;

    let period_frames: usize;

    {
        let hwp = HwParams::any(&pcm)?;

        hwp.set_access(Access::RWInterleaved)?;
        hwp.set_format(Format::s16())?;
        hwp.set_channels(CHANNELS as u32)?;
        hwp.set_rate(RATE as u32, ValueOr::Nearest)?;

        // Wunsch: kleine Perioden (z.B. ~10 ms)
        let period = hwp.set_period_size_near(480, ValueOr::Nearest)?;
        let buffer = hwp.set_buffer_size_near(period * 4)?;

        pcm.hw_params(&hwp)?;

        println!(
            "[alsa] rate={} period={} buffer={}",
            RATE, period, buffer
        );

        period_frames = period as usize;
    }

    pcm.prepare()?;
    let io = pcm.io_i16()?;

    // ------------------------------------------------------------
    // FIFO + Arbeitsbuffer
    // ------------------------------------------------------------
    let mut fifo: Vec<i16> = Vec::with_capacity(TARGET_FRAMES * CHANNELS * 2);
    let mut period_buf = vec![0i16; period_frames * CHANNELS];

    loop {
        match io.readi(&mut period_buf) {
            Ok(frames) if frames > 0 => {
                let samples = frames * CHANNELS;
                fifo.extend_from_slice(&period_buf[..samples]);

                while fifo.len() >= TARGET_FRAMES * CHANNELS {
                    if fifo.len() > TARGET_FRAMES * CHANNELS * 4 {
                        eprintln!("[alsa] fifo overrun, dropping");
                        fifo.clear();
                    }
                    let pcm_chunk: Vec<i16> =
                        fifo.drain(..TARGET_FRAMES * CHANNELS).collect();

                    let utc = utc_ns_now() - 100_000_000; // 100 ms
                    let seq = ring.writer_push(utc, pcm_chunk);

                    // Update metrics
                    metrics.alsa_samples.fetch_add(TARGET_FRAMES as u64 * CHANNELS as u64, Ordering::Relaxed);
                    
                    if seq % 10 == 0 {
                        println!("[alsa] pushed seq={}", seq);
                    }
                }
            }
            Ok(_) => {
                thread::sleep(Duration::from_millis(1));
            }
            Err(e) => {
                eprintln!("[alsa] read error: {}", e);
                thread::sleep(Duration::from_millis(10));
            }
        }
    }
}
