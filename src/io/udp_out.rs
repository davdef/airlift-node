// src/io/udp_out.rs
use crate::codecs::AudioCodec;
use crate::codecs::registry::{CodecRegistry, DEFAULT_CODEC_PCM_ID};
use crate::ring::{RingRead, RingReader};
use crate::monitoring::Metrics;
use byteorder::{BigEndian, WriteBytesExt};
use std::net::UdpSocket;
use std::time::{Duration, Instant};
use std::sync::Arc;
use std::sync::atomic::Ordering;

const TARGET_FRAMES: usize = 4800; // 100 ms @ 48 kHz
const CHANNELS: usize = 2;
const PKT: usize = 1316;           // MTU-/TS-freundlich
const MAGIC: &[u8; 4] = b"RFMA";   // wie vorher

pub fn run_udp_out(
    mut r: RingReader,
    target: &str,
    codec_id: Option<&str>,
    metrics: Arc<Metrics>,
    codec_registry: Arc<CodecRegistry>,
) -> std::io::Result<()> {
    let sock = UdpSocket::bind("0.0.0.0:0")?;
    sock.set_nonblocking(true)?;
    sock.connect(target)?;

    let mut sent_chunks: u64 = 0;
    let codec_id = codec_id.unwrap_or(DEFAULT_CODEC_PCM_ID);
    let (mut codec, codec_instance) = codec_registry
        .build_codec(codec_id)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    codec_instance.mark_ready();

    // --- Stats ---
    let mut gaps: u64 = 0;
    let mut missed_total: u64 = 0;
    let mut last_log = Instant::now();

    loop {
        match r.poll() {
            RingRead::Chunk(slot) => {
                // Erwartet: exakt 100 ms
                let needed = TARGET_FRAMES * CHANNELS;
                if slot.pcm.len() != needed {
                    eprintln!(
                        "[udp] unexpected pcm len={} (want {})",
                        slot.pcm.len(),
                        needed
                    );
                    continue;
                }

                let frames = match codec.encode(&slot.pcm) {
                    Ok(frames) => frames,
                    Err(e) => {
                        codec_instance.mark_error(&e.to_string());
                        eprintln!("[udp] codec error: {}", e);
                        continue;
                    }
                };
                let mut bytes = 0u64;
                let frame_count = frames.len() as u64;

                for frame in frames {
                    let pcm_bytes = frame.payload.len() as u32;

                    // Frame: magic(4) + seq(u64) + utc_ns(u64) + len(u32) + pcm(bytes)
                    let mut frame_buf =
                        Vec::with_capacity(4 + 8 + 8 + 4 + frame.payload.len());
                    frame_buf.extend_from_slice(MAGIC);
                    frame_buf.write_u64::<BigEndian>(slot.seq).unwrap();
                    frame_buf.write_u64::<BigEndian>(slot.utc_ns).unwrap();
                    frame_buf.write_u32::<BigEndian>(pcm_bytes).unwrap();

                    frame_buf.extend_from_slice(&frame.payload);
                    bytes += frame.payload.len() as u64;

                    // In 1316er Stücke senden
                    let mut ok = true;
                    for part in frame_buf.chunks(PKT) {
                        if let Err(e) = sock.send(part) {
                            eprintln!("[udp] send error: {}", e);
                            ok = false;
                            break;
                        }
                    }

                    if ok {
                        sent_chunks += 1;
                        metrics.udp_packets.fetch_add(1, Ordering::Relaxed);
                        metrics.bytes_sent.fetch_add(frame_buf.len() as u64, Ordering::Relaxed);

                        if sent_chunks % 10 == 0 {
                            println!(
                                "[udp] sent {} RFMA chunks (100 ms)",
                                sent_chunks
                            );
                        }
                    }
                }
                codec_instance.mark_encoded(1, frame_count, bytes);
            }

            RingRead::Gap { missed } => {
                gaps += 1;
                missed_total += missed;
                metrics.gaps_total.fetch_add(missed, Ordering::Relaxed);
            }

            RingRead::Empty => {
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
