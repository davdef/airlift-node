// src/io/alsa_in.rs

#![cfg(any(feature = "audio", feature = "mock-audio"))]

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::codecs::pcm::PcmPassthroughDecoder;
use crate::control::ModuleState;
use crate::decoder::AudioDecoder;
use crate::monitoring::Metrics;
use crate::ring::{AudioRing, PcmSink};

const RATE: usize = 48_000;
const CHANNELS: usize = 2;
const TARGET_FRAMES: usize = 4_800; // 100 ms

fn utc_ns_now() -> u64 {
    let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    d.as_secs() * 1_000_000_000 + d.subsec_nanos() as u64
}

// ============================================================
// REAL ALSA INPUT
// ============================================================

#[cfg(feature = "audio")]
use alsa::pcm::{Access, Format, HwParams, PCM};
#[cfg(feature = "audio")]
use alsa::{Direction, ValueOr};

#[cfg(feature = "audio")]
pub fn run_alsa_in(
    ring: AudioRing,
    metrics: Arc<Metrics>,
    state: Arc<ModuleState>,
    ring_state: Arc<ModuleState>,
) -> anyhow::Result<()> {
    state.set_running(true);
    state.set_connected(true);
    let pcm = PCM::new("default", Direction::Capture, false)?;

    let period_frames: usize;

    {
        let hwp = HwParams::any(&pcm)?;

        hwp.set_access(Access::RWInterleaved)?;
        hwp.set_format(Format::s16())?;
        hwp.set_channels(CHANNELS as u32)?;
        hwp.set_rate(RATE as u32, ValueOr::Nearest)?;

        let period = hwp.set_period_size_near(480, ValueOr::Nearest)?;
        let buffer = hwp.set_buffer_size_near(period * 4)?;

        pcm.hw_params(&hwp)?;

        println!("[alsa] rate={} period={} buffer={}", RATE, period, buffer);

        period_frames = period as usize;
    }

    pcm.prepare()?;
    let io = pcm.io_i16()?;

    let mut fifo: Vec<i16> = Vec::with_capacity(TARGET_FRAMES * CHANNELS * 2);
    let mut period_buf = vec![0i16; period_frames * CHANNELS];
    let mut decoder = PcmPassthroughDecoder::new(utc_ns_now());

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

                    let pcm_chunk: Vec<i16> = fifo.drain(..TARGET_FRAMES * CHANNELS).collect();

                    let utc = utc_ns_now() - 100_000_000;
                    decoder.set_next_timestamp(utc);
                    match decode_pcm_chunk(&mut decoder, &pcm_chunk) {
                        Ok(Some(frame)) => {
                            let seq = ring.writer_push(frame.utc_ns, frame.pcm);

                            metrics.alsa_samples.fetch_add(
                                TARGET_FRAMES as u64 * CHANNELS as u64,
                                Ordering::Relaxed,
                            );
                            state.mark_rx(TARGET_FRAMES as u64);
                            ring_state.mark_rx(1);

                            if seq % 10 == 0 {
                                println!("[alsa] pushed seq={}", seq);
                            }
                        }
                        Ok(None) => {}
                        Err(e) => {
                            eprintln!("[alsa] decode error: {}", e);
                        }
                    }
                }
            }
            Ok(_) => thread::sleep(Duration::from_millis(1)),
            Err(e) => {
                eprintln!("[alsa] read error: {}", e);
                thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

fn decode_pcm_chunk(
    decoder: &mut PcmPassthroughDecoder,
    pcm_chunk: &[i16],
) -> anyhow::Result<Option<crate::ring::PcmFrame>> {
    let mut payload = Vec::with_capacity(pcm_chunk.len() * 2);
    for sample in pcm_chunk {
        payload.extend_from_slice(&sample.to_le_bytes());
    }
    decoder.decode(&payload)
}

// ============================================================
// MOCK INPUT (for Codex / CI / non-ALSA builds)
// ============================================================

#[cfg(feature = "mock-audio")]
pub fn run_alsa_in(
    _ring: AudioRing,
    _metrics: Arc<Metrics>,
    state: Arc<ModuleState>,
    _ring_state: Arc<ModuleState>,
) -> anyhow::Result<()> {
    log::warn!("[mock-audio] ALSA input disabled (no audio source)");
    state.set_running(true);
    state.set_connected(false);

    // absichtlich nichts tun
    loop {
        thread::sleep(Duration::from_secs(1));
    }
}
