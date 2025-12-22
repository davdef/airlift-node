use std::io::{BufReader, Read};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use log::{info, warn};

use crate::codecs::{CodecInfo, CodecKind, ContainerKind, EncodedFrame};
use crate::control::ModuleState;
use crate::ring::EncodedRing;

const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
const READ_BUFFER_SIZE: usize = 64 * 1024;

pub fn run_icecast_in(
    ring: EncodedRing,
    url: String,
    running: Arc<AtomicBool>,
    state: Arc<ModuleState>,
    ring_state: Arc<ModuleState>,
) -> Result<()> {
    state.set_running(true);
    state.set_connected(false);

    let mut backoff = INITIAL_BACKOFF;
    while running.load(Ordering::Relaxed) {
        match connect_and_stream(&ring, &url, running.clone(), state.clone(), ring_state.clone()) {
            Ok(()) => {
                state.set_connected(false);
                backoff = INITIAL_BACKOFF;
            }
            Err(e) => {
                warn!("[icecast_in] error: {}", e);
                state.mark_error(1);
                state.set_connected(false);
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
        }
    }

    state.set_running(false);
    state.set_connected(false);
    Ok(())
}

fn connect_and_stream(
    ring: &EncodedRing,
    url: &str,
    running: Arc<AtomicBool>,
    state: Arc<ModuleState>,
    ring_state: Arc<ModuleState>,
) -> Result<()> {
    info!("[icecast_in] connecting to {}", url);
    let response = ureq::get(url)
        .timeout_read(Duration::from_secs(10))
        .timeout_connect(Duration::from_secs(5))
        .call()
        .map_err(|e| anyhow!("http error: {}", e))?;

    if response.status() >= 400 {
        return Err(anyhow!(
            "http status {} {}",
            response.status(),
            response.status_text()
        ));
    }

    state.set_connected(true);

    let mut reader = BufReader::with_capacity(READ_BUFFER_SIZE, response.into_reader());
    let mut packet_assembler = OggPacketAssembler::default();
    let mut codec_info: Option<CodecInfo> = None;
    let mut pending_pages: Vec<Vec<u8>> = Vec::new();

    while running.load(Ordering::Relaxed) {
        let page = match read_ogg_page(&mut reader)? {
            Some(page) => page,
            None => return Err(anyhow!("stream ended")),
        };

        if codec_info.is_none() {
            for packet in packet_assembler.push_page(&page) {
                if let Some(info) = parse_opus_head(&packet) {
                    codec_info = Some(info);
                    break;
                }
            }
        } else {
            packet_assembler.push_page(&page);
        }

        if let Some(info) = &codec_info {
            if !pending_pages.is_empty() {
                for pending in pending_pages.drain(..) {
                    push_frame(ring, info, pending, &state, &ring_state);
                }
            }
            push_frame(ring, info, page, &state, &ring_state);
        } else {
            pending_pages.push(page);
        }
    }

    Ok(())
}

fn push_frame(
    ring: &EncodedRing,
    info: &CodecInfo,
    payload: Vec<u8>,
    state: &ModuleState,
    ring_state: &ModuleState,
) {
    let utc_ns = now_utc_ns();
    ring.writer_push(
        utc_ns,
        EncodedFrame {
            payload,
            info: info.clone(),
        },
    );
    state.mark_rx(1);
    ring_state.mark_rx(1);
}

fn read_ogg_page(reader: &mut impl Read) -> Result<Option<Vec<u8>>> {
    let mut header = [0u8; 27];
    if let Err(err) = reader.read_exact(&mut header) {
        if err.kind() == std::io::ErrorKind::UnexpectedEof {
            return Ok(None);
        }
        return Err(err).context("failed to read ogg header");
    }

    if &header[..4] != b"OggS" {
        return Err(anyhow!("invalid ogg capture pattern"));
    }

    let seg_count = header[26] as usize;
    let mut segment_table = vec![0u8; seg_count];
    reader
        .read_exact(&mut segment_table)
        .context("failed to read ogg segment table")?;

    let body_size: usize = segment_table.iter().map(|v| *v as usize).sum();
    let mut body = vec![0u8; body_size];
    reader
        .read_exact(&mut body)
        .context("failed to read ogg page body")?;

    let mut page = Vec::with_capacity(27 + seg_count + body_size);
    page.extend_from_slice(&header);
    page.extend_from_slice(&segment_table);
    page.extend_from_slice(&body);
    Ok(Some(page))
}

#[derive(Default)]
struct OggPacketAssembler {
    buffer: Vec<u8>,
}

impl OggPacketAssembler {
    fn push_page(&mut self, page: &[u8]) -> Vec<Vec<u8>> {
        if page.len() < 27 {
            return Vec::new();
        }

        let seg_count = page[26] as usize;
        if page.len() < 27 + seg_count {
            return Vec::new();
        }

        let segment_table = &page[27..27 + seg_count];
        let mut offset = 27 + seg_count;
        let mut packets = Vec::new();

        for seg_len in segment_table.iter().copied() {
            let seg_len = seg_len as usize;
            if offset + seg_len > page.len() {
                break;
            }
            self.buffer.extend_from_slice(&page[offset..offset + seg_len]);
            offset += seg_len;

            if seg_len < 255 {
                packets.push(std::mem::take(&mut self.buffer));
            }
        }

        packets
    }
}

fn parse_opus_head(packet: &[u8]) -> Option<CodecInfo> {
    if packet.len() < 19 {
        return None;
    }
    if !packet.starts_with(b"OpusHead") {
        return None;
    }
    let channels = packet[9];
    if channels == 0 {
        return None;
    }

    Some(CodecInfo {
        kind: CodecKind::OpusOgg,
        sample_rate: 48_000,
        channels,
        container: ContainerKind::Ogg,
    })
}

fn now_utc_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}
