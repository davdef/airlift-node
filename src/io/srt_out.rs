use crate::codecs::registry::CodecRegistry;
use crate::codecs::{CodecInfo, ContainerKind, EncodedFrame};
use crate::config::SrtOutConfig;
use anyhow::Result;

use srt_tokio::SrtSocket;
use tokio::runtime::Runtime;

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use bytes::Bytes;
use futures_util::SinkExt;
use tokio::time::sleep;

use crate::control::{ModuleState, SrtOutState};

const INITIAL_BACKOFF_MS: u64 = 250;
const MAX_BACKOFF_MS: u64 = 5000;
const SHUTDOWN_POLL_MS: u64 = 100;

pub enum EncodedRead {
    Frame(EncodedFrame),
    Gap { missed: u64 },
    Empty,
}

pub trait EncodedFrameSource: Send {
    fn poll(&mut self) -> anyhow::Result<EncodedRead>;
}

pub fn run_srt_out(
    mut reader: impl EncodedFrameSource + 'static,
    cfg: SrtOutConfig,
    running: Arc<AtomicBool>,
    state: Arc<SrtOutState>,
    ring_state: Arc<ModuleState>,
    codec_registry: Arc<CodecRegistry>,
) -> Result<()> {
    let codec_id = require_codec_id(cfg.codec_id.as_deref())?;
    let codec_info = codec_registry.get_info(codec_id)?;
    validate_srt_codec(codec_id, &codec_info)?;
    let rt = Runtime::new()?;
    rt.block_on(async move { async_run(&mut reader, cfg, running, state, ring_state).await })
}

async fn async_run(
    reader: &mut impl EncodedFrameSource,
    cfg: SrtOutConfig,
    running: Arc<AtomicBool>,
    state: Arc<SrtOutState>,
    ring_state: Arc<ModuleState>,
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
        )
        .await
        {
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
    reader: &mut impl EncodedFrameSource,
    cfg: &SrtOutConfig,
    running: Arc<AtomicBool>,
    state: Arc<SrtOutState>,
    ring_state: Arc<ModuleState>,
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

    while running.load(Ordering::Relaxed) {
        if state.force_reconnect.swap(false, Ordering::Relaxed) {
            eprintln!("[srt_out] reconnect requested");
            state.module.mark_drop(1);
            state.module.set_connected(false);
            return Ok(());
        }
        match reader.poll()? {
            EncodedRead::Frame(frame) => {
                tx.send((Instant::now(), Bytes::from(frame.payload)))
                    .await?;
                state.module.mark_tx(1);
                ring_state.mark_tx(1);
            }

            EncodedRead::Gap { missed } => {
                eprintln!("[srt_out] GAP: missed {}", missed);
                state.module.mark_drop(missed);
                ring_state.mark_drop(missed);
            }

            EncodedRead::Empty => {
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

fn require_codec_id(codec_id: Option<&str>) -> Result<&str> {
    codec_id.ok_or_else(|| anyhow::anyhow!("missing codec_id for srt output"))
}

fn validate_srt_codec(codec_id: &str, info: &CodecInfo) -> Result<()> {
    match info.container {
        ContainerKind::Raw | ContainerKind::Ogg | ContainerKind::Mpeg => Ok(()),
        ContainerKind::Rtp => Err(anyhow::anyhow!(
            "srt output does not accept RTP container (codec_id '{}', container {:?})",
            codec_id,
            info.container
        )),
    }
}

impl EncodedFrameSource for crate::ring::RingReader {
    fn poll(&mut self) -> anyhow::Result<EncodedRead> {
        Err(anyhow::anyhow!(
            "PCM ring reader not supported for encoded outputs"
        ))
    }
}
