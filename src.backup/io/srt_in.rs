use crate::codecs::pcm::PcmPassthroughDecoder;
use crate::config::SrtInConfig;
use crate::container::parse_packet as parse_rfma_packet;
use crate::decoder::AudioDecoder;
use crate::ring::{AudioRing, PcmFrame, PcmSink};
use anyhow::Result;

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use futures_util::TryStreamExt;
use srt_tokio::SrtSocket;
use tokio::runtime::Runtime;
use tokio::time::{sleep, timeout};

use crate::control::{ModuleState, SrtInState};

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
    let mut decoder = PcmPassthroughDecoder::new(0);

    while running.load(Ordering::Relaxed) {
        if state.force_disconnect.swap(false, Ordering::Relaxed) {
            println!("[srt_in] forced disconnect (logical)");
            state.module.mark_drop(1);
            state.module.set_connected(false);
        }

        match timeout(INACTIVITY_TIMEOUT, rx.try_next()).await {
            Ok(Ok(Some((_inst, msg)))) => {
                state.module.set_connected(true);

                match parse_rfma_packet(msg.as_ref()) {
                    Ok(packet) => {
                        decoder.set_next_timestamp(packet.utc_ns);
                        match decoder.decode(&packet.payload) {
                            Ok(Some(frame)) => {
                                if let Err(e) = handle_pcm_frame(&ring, frame) {
                                    eprintln!("[srt_in] frame error: {e}");
                                    state.module.mark_drop(1);
                                } else {
                                    state.module.mark_rx(1);
                                    ring_state.mark_rx(1);
                                    frames_minute += 1;
                                }
                            }
                            Ok(None) => {}
                            Err(e) => {
                                eprintln!("[srt_in] frame error: {e}");
                                state.module.mark_drop(1);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("[srt_in] frame error: {e}");
                        state.module.mark_drop(1);
                    }
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

fn handle_pcm_frame<S: PcmSink>(sink: &S, frame: PcmFrame) -> Result<()> {
    sink.push(frame)
}
