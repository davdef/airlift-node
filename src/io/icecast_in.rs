// src/io/icecast_in.rs

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Result, anyhow};
use log::{info, warn, error};
use opus::{Decoder as OpusDecoder, Channels as OpusChannels};
use std::io::{Read, BufReader};

use crate::codecs::{PCM_I16_SAMPLES, PCM_SAMPLE_RATE};
use crate::control::ModuleState;
use crate::ring::AudioRing;

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

struct OggStreamParser<R: Read> {
    reader: R,
    buffer: Vec<u8>,
    current_page_pos: usize,
    segments_in_current_page: Vec<u8>,
    current_segment_idx: usize,
    packet_accumulator: Vec<u8>,
}

impl<R: Read> OggStreamParser<R> {
    fn new(reader: R) -> Self {
        Self {
            reader,
            buffer: Vec::with_capacity(65536),
            current_page_pos: 0,
            segments_in_current_page: Vec::new(),
            current_segment_idx: 0,
            packet_accumulator: Vec::new(),
        }
    }
    
    fn next_packet(&mut self) -> Result<Option<Vec<u8>>> {
        loop {
            // Wenn wir eine aktive Seite haben, extrahiere das nächste Paket
            if !self.segments_in_current_page.is_empty() && 
               self.current_segment_idx < self.segments_in_current_page.len() {
                if let Some(packet) = self.extract_next_packet() {
                    return Ok(Some(packet));
                }
            }
            
            // Finde die nächste OGG-Seite
            match self.find_and_parse_next_page() {
                Ok(true) => {
                    // Neue Seite geladen, weiter im Loop
                    continue;
                }
                Ok(false) => {
                    // Stream Ende
                    return Ok(None);
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }
    }
    
    fn find_and_parse_next_page(&mut self) -> Result<bool> {
        // Suche nach OGG-Seite im Buffer
        loop {
            // Finde "OggS" im Buffer
            for i in self.current_page_pos..self.buffer.len().saturating_sub(27) {
                if self.buffer[i..].starts_with(&[0x4f, 0x67, 0x67, 0x53]) {
                    // Mögliche OGG-Seite gefunden
                    if let Some(page_info) = self.parse_page_at(i) {
                        // Seite erfolgreich geparsed
                        self.current_page_pos = i + page_info.total_size;
                        self.segments_in_current_page = page_info.segment_table;
                        self.current_segment_idx = 0;
                        return Ok(true);
                    }
                }
            }
            
            // Keine Seite gefunden, lese mehr Daten
            let mut temp_buf = [0u8; 8192];
            match self.reader.read(&mut temp_buf) {
                Ok(0) => {
                    // Stream Ende
                    return Ok(false);
                }
                Ok(n) => {
                    self.buffer.extend_from_slice(&temp_buf[..n]);
                    
                    // Buffer begrenzen
                    if self.buffer.len() > 131072 {
                        // Entferne verarbeitete Daten
                        if self.current_page_pos > 65536 {
                            let to_remove = self.current_page_pos - 32768;
                            self.buffer.drain(..to_remove);
                            self.current_page_pos -= to_remove;
                        }
                    }
                }
                Err(e) => {
                    return Err(anyhow!("Read error: {}", e));
                }
            }
        }
    }
    
    fn parse_page_at(&self, start: usize) -> Option<PageInfo> {
        if start + 27 > self.buffer.len() {
            return None;
        }
        
        // Lese OGG Header
        let capture_pattern = &self.buffer[start..start+4];
        if capture_pattern != b"OggS" {
            return None;
        }
        
        let version = self.buffer[start + 4];
        let header_type = self.buffer[start + 5];
        let _granule_position = u64::from_le_bytes([
            self.buffer[start + 6], self.buffer[start + 7],
            self.buffer[start + 8], self.buffer[start + 9],
            self.buffer[start + 10], self.buffer[start + 11],
            self.buffer[start + 12], self.buffer[start + 13],
        ]);
        let _bitstream_serial = u32::from_le_bytes([
            self.buffer[start + 14], self.buffer[start + 15],
            self.buffer[start + 16], self.buffer[start + 17],
        ]);
        let _page_sequence = u32::from_le_bytes([
            self.buffer[start + 18], self.buffer[start + 19],
            self.buffer[start + 20], self.buffer[start + 21],
        ]);
        let _checksum = u32::from_le_bytes([
            self.buffer[start + 22], self.buffer[start + 23],
            self.buffer[start + 24], self.buffer[start + 25],
        ]);
        let page_segments = self.buffer[start + 26] as usize;
        
        // Prüfe ob Segment-Tabelle komplett ist
        if start + 27 + page_segments > self.buffer.len() {
            return None;
        }
        
        // Lese Segment-Tabelle
        let segment_table_start = start + 27;
        let mut segment_table = Vec::with_capacity(page_segments);
        let mut total_segment_size = 0;
        
        for i in 0..page_segments {
            let seg_size = self.buffer[segment_table_start + i];
            segment_table.push(seg_size);
            total_segment_size += seg_size as usize;
        }
        
        // Prüfe ob alle Segment-Daten da sind
        let page_start = start;
        let page_end = start + 27 + page_segments + total_segment_size;
        
        if page_end > self.buffer.len() {
            return None;
        }
        
        Some(PageInfo {
            start: page_start,
            total_size: page_end - page_start,
            segment_table,
            header_type,
        })
    }
    
    fn extract_next_packet(&mut self) -> Option<Vec<u8>> {
        while self.current_segment_idx < self.segments_in_current_page.len() {
            let segment_size = self.segments_in_current_page[self.current_segment_idx] as usize;
            
            // Berechne Position des Segments in der aktuellen Seite
            let page_start = self.current_page_pos - self.segments_in_current_page.iter()
                .map(|&s| s as usize)
                .sum::<usize>() - 27 - self.segments_in_current_page.len();
            
            let segment_start = page_start + 27 + self.segments_in_current_page.len() + 
                self.segments_in_current_page[..self.current_segment_idx]
                    .iter()
                    .map(|&s| s as usize)
                    .sum::<usize>();
            
            if segment_start + segment_size <= self.buffer.len() {
                let segment_data = &self.buffer[segment_start..segment_start + segment_size];
                self.packet_accumulator.extend_from_slice(segment_data);
            }
            
            self.current_segment_idx += 1;
            
            // Wenn dies das letzte Segment ist ODER Segment-Größe < 255,
            // dann ist das Paket komplett
            let is_last_segment = self.current_segment_idx == self.segments_in_current_page.len();
            let segment_completes_packet = segment_size < 255;
            
            if is_last_segment || segment_completes_packet {
                if !self.packet_accumulator.is_empty() {
                    let packet = self.packet_accumulator.clone();
                    self.packet_accumulator.clear();
                    return Some(packet);
                }
            }
            
            // Wenn wir die letzte Segment erreicht haben, reset für nächste Seite
            if is_last_segment {
                self.segments_in_current_page.clear();
                self.current_segment_idx = 0;
            }
        }
        
        None
    }
}

struct PageInfo {
    start: usize,
    total_size: usize,
    segment_table: Vec<u8>,
    header_type: u8,
}

// ============================================================================
// Haupt-Decoding-Loop mit OGG-Parser
// ============================================================================

fn stream_with_ogg_parser<R: Read>(
    reader: R,
    ring: &AudioRing,
    running: Arc<AtomicBool>,
    state: Arc<ModuleState>,
    ring_state: Arc<ModuleState>,
) -> Result<()> {
    let mut parser = OggStreamParser::new(reader);
    let mut opus_decoder: Option<OpusDecoder> = None;
    let mut pcm_buffer: Vec<i16> = Vec::with_capacity(PCM_I16_SAMPLES * 2);
    let mut chunk_start_ns = now_utc_ns();
    let mut chunk_counter = 0;
    let mut channels = 2;
    let mut packets_processed = 0;
    
    info!("[icecast_in] starting OGG/Opus decoding with parser");
    
    while running.load(Ordering::Relaxed) {
        match parser.next_packet() {
            Ok(Some(packet)) => {
                packets_processed += 1;
                
                // Opus Header erkennen
                if packet.starts_with(b"OpusHead") && packet.len() >= 19 {
                    channels = packet[9] as usize;
                    opus_decoder = Some(OpusDecoder::new(
                        PCM_SAMPLE_RATE,
                        if channels == 1 { 
                            OpusChannels::Mono 
                        } else { 
                            OpusChannels::Stereo 
                        }
                    ).map_err(|e| anyhow!("Failed to create Opus decoder: {}", e))?);
                    
                    info!("[icecast_in] Opus decoder: {} channels, {} Hz", 
                          channels, PCM_SAMPLE_RATE);
                    continue;
                }
                
                // Tags ignorieren
                if packet.starts_with(b"OpusTags") {
                    continue;
                }
                
                // Audio-Pakete dekodieren
                if let Some(ref mut decoder) = opus_decoder {
                    let mut decode_buf = vec![0i16; 5760 * 2];
                    
                    match decoder.decode(&packet, &mut decode_buf, false) {
                        Ok(frame_samples) => {
                            if frame_samples > 0 {
                                // Zu Stereo PCM konvertieren
                                let stereo_pcm = if channels == 1 {
                                    let mut stereo = Vec::with_capacity(frame_samples * 2);
                                    for &sample in &decode_buf[..frame_samples] {
                                        stereo.push(sample);
                                        stereo.push(sample);
                                    }
                                    stereo
                                } else {
                                    decode_buf[..frame_samples * 2].to_vec()
                                };
                                
                                pcm_buffer.extend_from_slice(&stereo_pcm);
                                
                                // 100ms-Chunks senden
                                while pcm_buffer.len() >= PCM_I16_SAMPLES {
                                    let chunk: Vec<i16> = pcm_buffer.drain(..PCM_I16_SAMPLES).collect();
                                    let timestamp = chunk_start_ns + (chunk_counter * CHUNK_DURATION_NS);
                                    
                                    ring.writer_push(timestamp, chunk.clone());
                                    state.mark_rx(1);
                                    ring_state.mark_rx(1);
                                    
                                    chunk_counter += 1;
                                    
                                    // Progress alle 10 Sekunden
                                    if chunk_counter % 100 == 0 {
                                        info!("[icecast_in] progress: {} chunks ({} seconds), {} packets", 
                                              chunk_counter, chunk_counter / 10, packets_processed);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            warn!("[icecast_in] decode error on packet {}: {}", packets_processed, e);
                            state.mark_error(1);
                        }
                    }
                } else {
                    // Noch kein Decoder
                    if packets_processed > 20 {
                        warn!("[icecast_in] still no decoder after {} packets", packets_processed);
                        return Err(anyhow!("No Opus decoder created"));
                    }
                }
            }
            Ok(None) => {
                info!("[icecast_in] stream ended after {} chunks, {} packets", 
                      chunk_counter, packets_processed);
                return Err(anyhow!("Stream ended"));
            }
            Err(e) => {
                error!("[icecast_in] parser error: {}", e);
                state.mark_error(1);
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
    
    info!("[icecast_in] stopped after {} chunks, {} packets", 
          chunk_counter, packets_processed);
    Ok(())
}

fn now_utc_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}
