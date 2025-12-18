use anyhow::{Result, anyhow};
use crate::ring::AudioRing;
use crate::config::SrtInConfig;

use byteorder::{BigEndian, LittleEndian, ReadBytesExt};
use std::io::{Cursor, Read};
use std::net::SocketAddr;
use std::time::Duration;

use tokio::runtime::Runtime;
use srt_tokio::SrtSocket;
use futures_util::TryStreamExt;

const MAGIC: &[u8; 4] = b"RFMA";

pub fn run_srt_in(ring: AudioRing, cfg: SrtInConfig) -> Result<()> {
    let rt = Runtime::new()?;
    rt.block_on(async move { async_run(ring, cfg).await })
}

async fn async_run(ring: AudioRing, cfg: SrtInConfig) -> Result<()> {
    let addr: SocketAddr = cfg.listen.parse()?;
    let port = addr.port();

    println!("[srt_in] listening on {} (port {})", cfg.listen, port);

    loop {
        println!("[srt_in] waiting for client …");

        let mut rx = SrtSocket::builder()
            .latency(Duration::from_millis(cfg.latency_ms as u64))
            .listen_on(port)
            .await?;

        println!("[srt_in] client connected");

        while let Some((_instant, msg)) = rx.try_next().await? {
            if let Err(e) = handle_rfma(&ring, &msg) {
                eprintln!("[srt_in] frame error: {e}");
            }
        }

        println!("[srt_in] client disconnected");
    }
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
