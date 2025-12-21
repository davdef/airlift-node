use crate::codecs::AudioCodec;
use crate::codecs::registry::{CodecRegistry, DEFAULT_CODEC_PCM_ID};
use crate::config::SrtOutConfig;
use crate::ring::{RingRead, RingReader};
use anyhow::Result;

use srt_tokio::SrtSocket;
use tokio::runtime::Runtime;

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use byteorder::{BigEndian, WriteBytesExt};
use bytes::Bytes;
use futures_util::SinkExt;
use tokio::time::sleep;

use crate::control::{ModuleState, SrtOutState};

const MAGIC: &[u8; 4] = b"RFMA";
const INITIAL_BACKOFF_MS: u64 = 250;
const MAX_BACKOFF_MS: u64 = 5000;
const SHUTDOWN_POLL_MS: u64 = 100;

pub fn run_srt_out(
    mut reader: RingReader,
    cfg: SrtOutConfig,
    running: Arc<AtomicBool>,
    state: Arc<SrtOutState>,
    ring_state: Arc<ModuleState>,
    codec_registry: Arc<CodecRegistry>,
) -> Result<()> {
    let rt = Runtime::new()?;
    rt.block_on(async move {
        async_run(&mut reader, cfg, running, state, ring_state, codec_registry).await
    })
}

async fn async_run(
    reader: &mut RingReader,
    cfg: SrtOutConfig,
    running: Arc<AtomicBool>,
    state: Arc<SrtOutState>,
    ring_state: Arc<ModuleState>,
    codec_registry: Arc<CodecRegistry>,
) -> Result<()> {
    let mut backoff_ms = INITIAL_BACKOFF_MS;
    state.module.set_running(true);
    state.module.set_connected(false);

    while running.load(Ordering::Relaxed) {
        println!("[srt_out] connecting to {} …", cfg.target);

        match connect_and_run(
            reader,
            &cfg,
            running.clone(),
            state.clone(),
            ring_state.clone(),
            codec_registry.clone(),
        )
        .await {
            Ok(_) => {
                eprintln!("[srt_out] connection ended");
                state.module.set_connected(false);
                backoff_ms = INITIAL_BACKOFF_MS;
            }
            Err(e) => {
                eprintln!("[srt_out] error: {e}");
                state.module.mark_error(1);
                state.module.set_connected(false);
                backoff_ms = (backoff_ms * 2).min(MAX_BACKOFF_MS);
            }
        }

        if !running.load(Ordering::Relaxed) {
            break;
        }

        sleep(Duration::from_millis(backoff_ms)).await;
    }

    state.module.set_running(false);
    state.module.set_connected(false);
    Ok(())
}

async fn connect_and_run(
    reader: &mut RingReader,
    cfg: &SrtOutConfig,
    running: Arc<AtomicBool>,
    state: Arc<SrtOutState>,
    ring_state: Arc<ModuleState>,
    codec_registry: Arc<CodecRegistry>,
) -> Result<()> {
    let addr: SocketAddr = cfg.target.parse()?;

    let connect_fut = SrtSocket::builder()
        .latency(Duration::from_millis(cfg.latency_ms as u64))
        .call(addr, None);

    let mut tx = tokio::select! {
        res = connect_fut => res?,
        _ = wait_for_shutdown(running.clone()) => {
            return Ok(());
        }
    };

    println!("[srt_out] connected");
    state.module.set_connected(true);

    let codec_id = cfg.codec_id.as_deref().unwrap_or(DEFAULT_CODEC_PCM_ID);
    let (mut codec, codec_instance) = codec_registry.build_codec(codec_id)?;
    codec_instance.mark_ready();

    while running.load(Ordering::Relaxed) {
        if state.force_reconnect.swap(false, Ordering::Relaxed) {
            eprintln!("[srt_out] reconnect requested");
            state.module.mark_drop(1);
            state.module.set_connected(false);
            return Ok(());
        }
        match reader.poll() {
            RingRead::Chunk(slot) => {
                let frames = codec.encode(&slot.pcm).map_err(|e| {
                    codec_instance.mark_error(&e.to_string());
                    e
                })?;
                let mut bytes = 0u64;
                let frame_count = frames.len() as u64;
                for frame in frames {
                    let pcm_bytes = frame.payload.len() as u32;

                    let mut buf = Vec::with_capacity(4 + 8 + 8 + 4 + frame.payload.len());

                    buf.extend_from_slice(MAGIC);
                    buf.write_u64::<BigEndian>(slot.seq)?;
                    buf.write_u64::<BigEndian>(slot.utc_ns)?;
                    buf.write_u32::<BigEndian>(pcm_bytes)?;
                    buf.extend_from_slice(&frame.payload);

                    tx.send((Instant::now(), Bytes::from(buf))).await?;
                    state.module.mark_tx(1);
                    ring_state.mark_tx(1);
                    bytes += frame.payload.len() as u64;
                }
                codec_instance.mark_encoded(1, frame_count, bytes);
            }

            RingRead::Gap { missed } => {
                eprintln!("[srt_out] GAP: missed {}", missed);
                state.module.mark_drop(missed);
                ring_state.mark_drop(missed);
            }

            RingRead::Empty => {
                sleep(Duration::from_millis(2)).await;
            }
        }
    }

    Ok(())
}

async fn wait_for_shutdown(running: Arc<AtomicBool>) {
    while running.load(Ordering::Relaxed) {
        sleep(Duration::from_millis(SHUTDOWN_POLL_MS)).await;
    }
}
