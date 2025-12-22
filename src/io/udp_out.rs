// src/io/udp_out.rs
use crate::codecs::registry::CodecRegistry;
use crate::codecs::{CodecInfo, ContainerKind, EncodedFrame};
use crate::monitoring::Metrics;
use std::net::UdpSocket;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

const PKT: usize = 1316; // MTU-/TS-freundlich

pub enum EncodedRead {
    Frame(EncodedFrame),
    Gap { missed: u64 },
    Empty,
}

pub trait EncodedFrameSource: Send {
    fn poll(&mut self) -> anyhow::Result<EncodedRead>;
    fn fill(&self) -> u64 {
        0
    }
}

pub fn run_udp_out(
    mut r: impl EncodedFrameSource + 'static,
    target: &str,
    codec_id: Option<&str>,
    metrics: Arc<Metrics>,
    codec_registry: Arc<CodecRegistry>,
) -> std::io::Result<()> {
    let codec_id = require_codec_id(codec_id)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    let codec_info = codec_registry
        .get_info(codec_id)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    validate_udp_codec(codec_id, &codec_info)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    let sock = UdpSocket::bind("0.0.0.0:0")?;
    sock.set_nonblocking(true)?;
    sock.connect(target)?;

    let mut sent_chunks: u64 = 0;

    // --- Stats ---
    let mut gaps: u64 = 0;
    let mut missed_total: u64 = 0;
    let mut last_log = Instant::now();

    loop {
        match r
            .poll()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?
        {
            EncodedRead::Frame(frame) => {
                let mut ok = true;
                for part in frame.payload.chunks(PKT) {
                    if let Err(e) = sock.send(part) {
                        eprintln!("[udp] send error: {}", e);
                        ok = false;
                        break;
                    }
                }

                if ok {
                    sent_chunks += 1;
                    metrics.udp_packets.fetch_add(1, Ordering::Relaxed);
                    metrics
                        .bytes_sent
                        .fetch_add(frame.payload.len() as u64, Ordering::Relaxed);

                    if sent_chunks % 10 == 0 {
                        println!("[udp] sent {} chunks", sent_chunks);
                    }
                }
            }

            EncodedRead::Gap { missed } => {
                gaps += 1;
                missed_total += missed;
                metrics.gaps_total.fetch_add(missed, Ordering::Relaxed);
            }

            EncodedRead::Empty => {
                std::thread::sleep(Duration::from_millis(2));
            }
        }

        // --- Periodisches Logging ---
        if last_log.elapsed() >= Duration::from_secs(5) {
            let fill = r.fill();

            eprintln!(
                "[udp] fill={} slots | sent={} | GAPs={} missed={}",
                fill, sent_chunks, gaps, missed_total
            );

            gaps = 0;
            missed_total = 0;
            sent_chunks = 0;
            last_log = Instant::now();
        }
    }
}

impl EncodedFrameSource for crate::ring::RingReader {
    fn poll(&mut self) -> anyhow::Result<EncodedRead> {
        Err(anyhow::anyhow!(
            "PCM ring reader not supported for encoded outputs"
        ))
    }
}

fn require_codec_id(codec_id: Option<&str>) -> anyhow::Result<&str> {
    codec_id.ok_or_else(|| anyhow::anyhow!("missing codec_id for udp output"))
}

fn validate_udp_codec(codec_id: &str, info: &CodecInfo) -> anyhow::Result<()> {
    match info.container {
        ContainerKind::Raw | ContainerKind::Rtp => Ok(()),
        ContainerKind::Ogg | ContainerKind::Mpeg => Err(anyhow::anyhow!(
            "udp output expects raw/rtp containers (codec_id '{}', container {:?})",
            codec_id,
            info.container
        )),
    }
}
