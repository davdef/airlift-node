use crate::config::SrtInConfig;
use crate::ring::AudioRing;
use anyhow::{Result, anyhow};

use byteorder::{BigEndian, LittleEndian, ReadBytesExt};
use std::io::{Cursor, Read};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use futures_util::TryStreamExt;
use srt_tokio::SrtSocket;
use tokio::runtime::Runtime;
use tokio::time::{sleep, timeout};

use crate::control::{ModuleState, SrtInState};

const MAGIC: &[u8; 4] = b"RFMA";
const INACTIVITY_TIMEOUT: Duration = Duration::from_secs(5);
const SHUTDOWN_POLL_MS: u64 = 100;
const STATS_INTERVAL: Duration = Duration::from_secs(60);

/* =========================
   ENTRY
   ========================= */

pub fn run_srt_in(
    ring: AudioRing,
    cfg: SrtInConfig,
    running: Arc<AtomicBool>,
    state: Arc<SrtInState>,
    ring_state: Arc<ModuleState>,
) -> Result<()> {
    let rt = Runtime::new()?;
    rt.block_on(async move { async_run(ring, cfg, running, state, ring_state).await })
}

async fn async_run(
    ring: AudioRing,
    cfg: SrtInConfig,
    running: Arc<AtomicBool>,
    state: Arc<SrtInState>,
    ring_state: Arc<ModuleState>,
) -> Result<()> {
    let addr: SocketAddr = cfg.listen.parse()?;

    println!("[srt_in] binding on {}", cfg.listen);
    state.module.set_running(true);
    state.module.set_connected(false);

    // 🔑 EINMAL binden
    let mut rx = SrtSocket::builder()
        .latency(Duration::from_millis(cfg.latency_ms as u64))
        .listen_on(addr)
        .await?;

    println!("[srt_in] ready, waiting for packets");

    let mut last_stats = Instant::now();
    let mut frames_minute: u64 = 0;

    while running.load(Ordering::Relaxed) {
        if state.force_disconnect.swap(false, Ordering::Relaxed) {
            println!("[srt_in] forced disconnect (logical)");
            state.module.mark_drop(1);
            state.module.set_connected(false);
        }

        match timeout(INACTIVITY_TIMEOUT, rx.try_next()).await {
            Ok(Ok(Some((_inst, msg)))) => {
                state.module.set_connected(true);

                if let Err(e) = handle_rfma(&ring, msg.as_ref()) {
                    eprintln!("[srt_in] frame error: {e}");
                    state.module.mark_drop(1);
                } else {
                    state.module.mark_rx(1);
                    ring_state.mark_rx(1);
                    frames_minute += 1;
                }
            }

            Ok(Ok(None)) => {
                // Peer weg, Listener bleibt
                if state.module.swap_connected(false) {
                    println!("[srt_in] client disconnected");
                }
            }

            Ok(Err(e)) => {
                eprintln!("[srt_in] receive error: {e}");
                state.module.mark_error(1);
                state.module.mark_drop(1);
                state.module.set_connected(false);
                sleep(Duration::from_millis(200)).await;
            }

            Err(_) => {
                if state.module.swap_connected(false) {
                    eprintln!("[srt_in] inactivity timeout");
                    state.module.mark_drop(1);
                }
            }
        }

        // 🕒 Minütliches Heartbeat-Log
        if last_stats.elapsed() >= STATS_INTERVAL {
            println!(
                "[srt_in] rx ok: {} frames/min (~{} fps)",
                frames_minute,
                frames_minute / 60
            );
            frames_minute = 0;
            last_stats = Instant::now();
        }
    }

    println!("[srt_in] stopped");
    state.module.set_running(false);
    state.module.set_connected(false);
    Ok(())
}

/* =========================
   RFMA
   ========================= */

fn handle_rfma(ring: &AudioRing, buf: &[u8]) -> Result<()> {
    let mut c = Cursor::new(buf);

    let mut magic = [0u8; 4];
    c.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(anyhow!("invalid RFMA magic"));
    }

    let _seq = c.read_u64::<BigEndian>()?;
    let utc_ns = c.read_u64::<BigEndian>()?;
    let pcm_len = c.read_u32::<BigEndian>()? as usize;

    if pcm_len % 2 != 0 {
        return Err(anyhow!("invalid pcm_len {}", pcm_len));
    }

    let samples = pcm_len / 2;
    let mut pcm = Vec::with_capacity(samples);
    for _ in 0..samples {
        pcm.push(c.read_i16::<LittleEndian>()?);
    }

    ring.writer_push(utc_ns, pcm);
    Ok(())
}
