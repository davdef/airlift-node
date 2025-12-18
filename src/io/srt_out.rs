use crate::config::SrtOutConfig;
use crate::ring::{RingRead, RingReader};
use anyhow::Result;

use srt_tokio::SrtSocket;
use tokio::runtime::Runtime;

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use byteorder::{BigEndian, LittleEndian, WriteBytesExt};
use bytes::Bytes;
use futures_util::SinkExt;
use tokio::time::sleep;

const MAGIC: &[u8; 4] = b"RFMA";
const INITIAL_BACKOFF_MS: u64 = 250;
const MAX_BACKOFF_MS: u64 = 5000;
const SHUTDOWN_POLL_MS: u64 = 100;

pub fn run_srt_out(
    mut reader: RingReader,
    cfg: SrtOutConfig,
    running: Arc<AtomicBool>,
) -> Result<()> {
    let rt = Runtime::new()?;
    rt.block_on(async move { async_run(&mut reader, cfg, running).await })
}

async fn async_run(
    reader: &mut RingReader,
    cfg: SrtOutConfig,
    running: Arc<AtomicBool>,
) -> Result<()> {
    let mut backoff_ms = INITIAL_BACKOFF_MS;

    while running.load(Ordering::Relaxed) {
        println!("[srt_out] connecting to {} …", cfg.target);

        match connect_and_run(reader, &cfg, running.clone()).await {
            Ok(_) => {
                eprintln!("[srt_out] connection ended");
                backoff_ms = INITIAL_BACKOFF_MS;
            }
            Err(e) => {
                eprintln!("[srt_out] error: {e}");
                backoff_ms = (backoff_ms * 2).min(MAX_BACKOFF_MS);
            }
        }

        if !running.load(Ordering::Relaxed) {
            break;
        }

        sleep(Duration::from_millis(backoff_ms)).await;
    }

    Ok(())
}

async fn connect_and_run(
    reader: &mut RingReader,
    cfg: &SrtOutConfig,
    running: Arc<AtomicBool>,
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

    while running.load(Ordering::Relaxed) {
        match reader.poll() {
            RingRead::Chunk(slot) => {
                let pcm = &slot.pcm;

                let mut buf = Vec::with_capacity(4 + 8 + 8 + 4 + pcm.len() * 2);

                buf.extend_from_slice(MAGIC);
                buf.write_u64::<BigEndian>(slot.seq)?;
                buf.write_u64::<BigEndian>(slot.utc_ns)?;
                buf.write_u32::<BigEndian>((pcm.len() * 2) as u32)?;

                for s in pcm.iter() {
                    buf.write_i16::<LittleEndian>(*s)?;
                }

                tx.send((Instant::now(), Bytes::from(buf))).await?;
            }

            RingRead::Gap { missed } => {
                eprintln!("[srt_out] GAP: missed {}", missed);
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
