// src/io/icecast_in.rs

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use log::{error, info, warn};
use opus::{Channels as OpusChannels, Decoder as OpusDecoder};
use std::io::{BufReader, Read};

use crate::codecs::{PCM_I16_SAMPLES, PCM_SAMPLE_RATE};
use crate::container::OggStreamParser;
use crate::control::ModuleState;
use crate::decoder::AudioDecoder;
use crate::ring::{AudioRing, PcmFrame, PcmSink};

const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
const READ_BUFFER_SIZE: usize = 64 * 1024;
const CHUNK_DURATION_NS: u64 = 100_000_000; // 100ms in nanoseconds

pub fn run_icecast_in(
    ring: AudioRing,
    url: String,
    running: Arc<AtomicBool>,
    state: Arc<ModuleState>,
    ring_state: Arc<ModuleState>,
) -> Result<()> {
    state.set_running(true);
    state.set_connected(false);

    let mut backoff = INITIAL_BACKOFF;

    while running.load(Ordering::Relaxed) {
        match connect_and_stream(
            &ring,
            &url,
            running.clone(),
            state.clone(),
            ring_state.clone(),
        ) {
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
    ring: &AudioRing,
    url: &str,
    running: Arc<AtomicBool>,
    state: Arc<ModuleState>,
    ring_state: Arc<ModuleState>,
) -> Result<()> {
    info!("[icecast_in] connecting to {}", url);

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(Duration::from_secs(10))
        .build();

    let response = agent
        .get(url)
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
    info!("[icecast_in] connected successfully");

    let reader = BufReader::with_capacity(READ_BUFFER_SIZE, response.into_reader());

    // Verwende den OGG-Parser
    stream_with_ogg_parser(reader, ring, running, state, ring_state)
}

// ============================================================================
// OGG-Parser mit korrekter Segment-Verarbeitung
// ============================================================================

fn stream_with_ogg_parser<R: Read, S: PcmSink>(
    reader: R,
    sink: &S,
    running: Arc<AtomicBool>,
    state: Arc<ModuleState>,
    ring_state: Arc<ModuleState>,
) -> Result<()> {
    let mut parser = OggStreamParser::new(reader);
    let mut decoder = OpusOggDecoder::new(now_utc_ns());
    let mut packets_processed = 0;

    info!("[icecast_in] starting OGG/Opus decoding with parser");

    while running.load(Ordering::Relaxed) {
        match parser.next_packet() {
            Ok(Some(packet)) => {
                packets_processed += 1;

                match decoder.decode(&packet) {
                    Ok(Some(frame)) => {
                        handle_pcm_frame(
                            sink,
                            frame,
                            &state,
                            &ring_state,
                            decoder.chunk_counter(),
                            packets_processed,
                        )?;
                        while let Some(frame) = decoder.take_pending_frame() {
                            handle_pcm_frame(
                                sink,
                                frame,
                                &state,
                                &ring_state,
                                decoder.chunk_counter(),
                                packets_processed,
                            )?;
                        }
                    }
                    Ok(None) => {}
                    Err(e) => {
                        warn!(
                            "[icecast_in] decode error on packet {}: {}",
                            packets_processed, e
                        );
                        state.mark_error(1);
                        if decoder.needs_decoder() && packets_processed > 20 {
                            warn!(
                                "[icecast_in] still no decoder after {} packets",
                                packets_processed
                            );
                            return Err(anyhow!("No Opus decoder created"));
                        }
                    }
                }
            }
            Ok(None) => {
                info!(
                    "[icecast_in] stream ended after {} chunks, {} packets",
                    decoder.chunk_counter(),
                    packets_processed
                );
                return Err(anyhow!("Stream ended"));
            }
            Err(e) => {
                error!("[icecast_in] parser error: {}", e);
                state.mark_error(1);
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

    info!(
        "[icecast_in] stopped after {} chunks, {} packets",
        decoder.chunk_counter(),
        packets_processed
    );
    Ok(())
}

fn now_utc_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

fn handle_pcm_frame<S: PcmSink>(
    sink: &S,
    frame: PcmFrame,
    state: &ModuleState,
    ring_state: &ModuleState,
    chunk_counter: u64,
    packets_processed: u64,
) -> Result<()> {
    sink.push(frame)?;
    state.mark_rx(1);
    ring_state.mark_rx(1);

    if chunk_counter % 100 == 0 {
        info!(
            "[icecast_in] progress: {} chunks ({} seconds), {} packets",
            chunk_counter,
            chunk_counter / 10,
            packets_processed
        );
    }
    Ok(())
}

struct OpusOggDecoder {
    opus_decoder: Option<OpusDecoder>,
    pcm_buffer: Vec<i16>,
    pending: std::collections::VecDeque<PcmFrame>,
    channels: usize,
    chunk_start_ns: u64,
    chunk_counter: u64,
}

impl OpusOggDecoder {
    fn new(chunk_start_ns: u64) -> Self {
        Self {
            opus_decoder: None,
            pcm_buffer: Vec::with_capacity(PCM_I16_SAMPLES * 2),
            pending: std::collections::VecDeque::new(),
            channels: 2,
            chunk_start_ns,
            chunk_counter: 0,
        }
    }

    fn chunk_counter(&self) -> u64 {
        self.chunk_counter
    }

    fn needs_decoder(&self) -> bool {
        self.opus_decoder.is_none()
    }

    fn take_pending_frame(&mut self) -> Option<PcmFrame> {
        self.pending.pop_front()
    }

    fn queue_frame(&mut self, pcm: Vec<i16>) {
        let timestamp = self.chunk_start_ns + (self.chunk_counter * CHUNK_DURATION_NS);
        self.chunk_counter += 1;
        self.pending.push_back(PcmFrame {
            utc_ns: timestamp,
            pcm,
        });
    }
}

impl AudioDecoder for OpusOggDecoder {
    fn decode(&mut self, packet: &[u8]) -> Result<Option<PcmFrame>> {
        if packet.starts_with(b"OpusHead") && packet.len() >= 19 {
            self.channels = packet[9] as usize;
            self.opus_decoder = Some(
                OpusDecoder::new(
                    PCM_SAMPLE_RATE,
                    if self.channels == 1 {
                        OpusChannels::Mono
                    } else {
                        OpusChannels::Stereo
                    },
                )
                .map_err(|e| anyhow!("Failed to create Opus decoder: {}", e))?,
            );
            info!(
                "[icecast_in] Opus decoder: {} channels, {} Hz",
                self.channels, PCM_SAMPLE_RATE
            );
            return Ok(None);
        }

        if packet.starts_with(b"OpusTags") {
            return Ok(None);
        }

        let Some(ref mut decoder) = self.opus_decoder else {
            return Ok(None);
        };

        let mut decode_buf = vec![0i16; 5760 * 2];
        let frame_samples = decoder.decode(packet, &mut decode_buf, false)?;
        if frame_samples == 0 {
            return Ok(None);
        }

        let stereo_pcm = if self.channels == 1 {
            let mut stereo = Vec::with_capacity(frame_samples * 2);
            for &sample in &decode_buf[..frame_samples] {
                stereo.push(sample);
                stereo.push(sample);
            }
            stereo
        } else {
            decode_buf[..frame_samples * 2].to_vec()
        };

        self.pcm_buffer.extend_from_slice(&stereo_pcm);
        while self.pcm_buffer.len() >= PCM_I16_SAMPLES {
            let chunk: Vec<i16> = self.pcm_buffer.drain(..PCM_I16_SAMPLES).collect();
            self.queue_frame(chunk);
        }

        Ok(self.pending.pop_front())
    }
}
