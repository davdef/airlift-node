use crate::config::SrtInConfig;
use crate::ring::AudioRing;
use anyhow::{Result, anyhow};

use byteorder::{BigEndian, LittleEndian, ReadBytesExt};
use std::io::{Cursor, Read};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures_util::TryStreamExt;
use srt_tokio::SrtSocket;
use tokio::runtime::Runtime;
use tokio::time::{sleep, timeout};

const MAGIC: &[u8; 4] = b"RFMA";
const INACTIVITY_TIMEOUT: Duration = Duration::from_secs(5);
const SHUTDOWN_POLL_MS: u64 = 100;

pub fn run_srt_in(ring: AudioRing, cfg: SrtInConfig, running: Arc<AtomicBool>) -> Result<()> {
    let rt = Runtime::new()?;
    rt.block_on(async move { async_run(ring, cfg, running).await })
}

async fn async_run(ring: AudioRing, cfg: SrtInConfig, running: Arc<AtomicBool>) -> Result<()> {
    let addr: SocketAddr = cfg.listen.parse()?;

    println!("[srt_in] listening on {}", cfg.listen);

    while running.load(Ordering::Relaxed) {
        println!("[srt_in] waiting for client …");

        let listen_fut = SrtSocket::builder()
            .latency(Duration::from_millis(cfg.latency_ms as u64))
            .listen_on(addr);

        let mut rx = tokio::select! {
            res = listen_fut => res?,
            _ = wait_for_shutdown(running.clone()) => {
                break;
            }
        };

        println!("[srt_in] client connected");

        while running.load(Ordering::Relaxed) {
            match timeout(INACTIVITY_TIMEOUT, rx.try_next()).await {
                Ok(Some((_instant, msg))) => {
                    if let Err(e) = handle_rfma(&ring, &msg) {
                        eprintln!("[srt_in] frame error: {e}");
                    }
                }
                Ok(None) => {
                    println!("[srt_in] client disconnected");
                    break;
                }
                Err(_) => {
                    eprintln!(
                        "[srt_in] receive timeout after {:?}, dropping client",
                        INACTIVITY_TIMEOUT
                    );
                    break;
                }
            }
        }

        println!("[srt_in] client disconnected");
    }

    Ok(())
}

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

async fn wait_for_shutdown(running: Arc<AtomicBool>) {
    while running.load(Ordering::Relaxed) {
        sleep(Duration::from_millis(SHUTDOWN_POLL_MS)).await;
    }
}
