use anyhow::Result;
use crate::ring::{RingReader, RingRead};
use crate::config::SrtOutConfig;

use tokio::runtime::Runtime;
use srt_tokio::SrtSocket;

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use bytes::Bytes;
use futures_util::SinkExt;
use byteorder::{BigEndian, LittleEndian, WriteBytesExt};

const MAGIC: &[u8; 4] = b"RFMA";

pub fn run_srt_out(mut reader: RingReader, cfg: SrtOutConfig) -> Result<()> {
    let rt = Runtime::new()?;
    rt.block_on(async move { async_run(&mut reader, cfg).await })
}

async fn async_run(reader: &mut RingReader, cfg: SrtOutConfig) -> Result<()> {
    let mut backoff_ms = 250u64;

    loop {
        println!("[srt_out] connecting to {} …", cfg.target);

        match connect_and_run(reader, &cfg).await {
            Ok(_) => eprintln!("[srt_out] connection ended"),
            Err(e) => eprintln!("[srt_out] error: {e}"),
        }

        tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
        backoff_ms = (backoff_ms * 2).min(5000);
    }
}

async fn connect_and_run(
    reader: &mut RingReader,
    cfg: &SrtOutConfig,
) -> Result<()> {
    let addr: SocketAddr = cfg.target.parse()?;

    let mut tx = SrtSocket::builder()
        .latency(Duration::from_millis(cfg.latency_ms as u64))
        .call(addr, None)
        .await?;

    println!("[srt_out] connected");

    loop {
        match reader.poll() {
            RingRead::Chunk(slot) => {
                let pcm = &slot.pcm;

                let mut buf = Vec::with_capacity(
                    4 + 8 + 8 + 4 + pcm.len() * 2
                );

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
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        }
    }
}
