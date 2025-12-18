// src/audio/timeshift.rs

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::thread::sleep;
use std::time::Duration;

use log::{debug, info, warn};

/// Audio-Parameter (müssen zu Recorder passen)
const SAMPLE_RATE: u64 = 48_000;
const CHANNELS: u64 = 2;
const BYTES_PER_SAMPLE: u64 = 2;
const FRAME_BYTES: u64 = CHANNELS * BYTES_PER_SAMPLE;

/// Blockierender Timeshift-Reader.
/// Ruft `on_pcm(bytes)` für jedes gelesene PCM-Chunk auf.
/// Kehrt **nur** bei Fehler oder Abbruch zurück.
pub fn stream_timeshift(
    wav_dir: PathBuf,
    start_ts_ms: u64,
    mut on_pcm: impl FnMut(&[u8]) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    info!("[timeshift] starting at ts={} ms", start_ts_ms);
    
    let mut cur_ts_ms = start_ts_ms;
    let mut retry_count = 0;

    loop {
        // aktuelle Stunde bestimmen
        let hour = (cur_ts_ms / 1000) / 3600;
        let hour_start_ms = hour * 3600 * 1000;

        let wav_path = wav_dir.join(format!("{}.wav", hour));
        info!("[timeshift] looking for hour {}: {:?}", hour, wav_path);

        // warten, bis Datei existiert (max 10s)
        while !wav_path.exists() {
            retry_count += 1;
            if retry_count > 50 { // 50 * 200ms = 10s
                warn!("[timeshift] file not found after 10s: {:?}", wav_path);
                return Ok(()); // Leeres Stream-Ende
            }
            sleep(Duration::from_millis(200));
        }
        
        info!("[timeshift] opening {:?}", wav_path);
        
        // Datei MANUELL öffnen und parsen
        let mut file = match File::open(&wav_path) {
            Ok(f) => f,
            Err(e) => {
                warn!("[timeshift] failed to open {:?}: {}", wav_path, e);
                sleep(Duration::from_millis(1000));
                continue;
            }
        };
        
        // 1. WAV-Header parsen
        let (data_start, file_size) = match parse_wav_header(&mut file) {
            Ok((start, size)) => {
                info!("[timeshift] WAV data starts at byte {}, file size {}", start, size);
                (start, size)
            }
            Err(e) => {
                warn!("[timeshift] invalid WAV header in {:?}: {}", wav_path, e);
                sleep(Duration::from_millis(1000));
                continue;
            }
        };
        
        // 2. Byte-Offset innerhalb dieser Stunde berechnen
        let offset_ms = cur_ts_ms.saturating_sub(hour_start_ms);
        let offset_frames = offset_ms * SAMPLE_RATE / 1000;
        let offset_bytes = offset_frames * FRAME_BYTES;
        
        info!("[timeshift] offset: {} ms = {} frames = {} bytes", 
              offset_ms, offset_frames, offset_bytes);
        
        // 3. Zu den Audio-Daten springen
        let target_pos = data_start + offset_bytes;
        let clamped_pos = if target_pos >= file_size {
            warn!("[timeshift] offset beyond file end, clamping");
            data_start
        } else {
            target_pos
        };
        
        if let Err(e) = file.seek(SeekFrom::Start(clamped_pos)) {
            warn!("[timeshift] seek failed: {}", e);
            continue;
        }
        
        info!("[timeshift] seeking to byte {}", clamped_pos);
        
        // 4. Streaming-Loop für diese Stunde
        let mut bytes_read_total = 0;
        let mut chunk_count = 0;
        
        loop {
            let mut buf = [0u8; 8192];
            let n = match file.read(&mut buf) {
                Ok(0) => {
                    // EOF erreicht
                    debug!("[timeshift] EOF for hour {}", hour);
                    
                    // Stundenwechsel erreicht?
                    if cur_ts_ms >= hour_start_ms + 3600 * 1000 {
                        info!("[timeshift] hour {} completed", hour);
                        break; // nächste Stunde öffnen
                    }
                    
                    // Sonst warten und von vorne lesen
                    sleep(Duration::from_millis(100));
                    
                    // Zurück zum Datenanfang dieser Datei
                    if let Err(e) = file.seek(SeekFrom::Start(data_start)) {
                        warn!("[timeshift] re-seek failed: {}", e);
                        break;
                    }
                    continue;
                }
                Ok(n) => n,
                Err(e) => {
                    warn!("[timeshift] read error: {}", e);
                    sleep(Duration::from_millis(100));
                    continue;
                }
            };
            
            if n > 0 {
                bytes_read_total += n;
                chunk_count += 1;
                
                // Nur alle 100 Chunks loggen
                if chunk_count % 100 == 0 {
                    info!("[timeshift] read {} bytes total ({} chunks)", 
                          bytes_read_total, chunk_count);
                }
                
                // PCM-Daten an Callback übergeben
                if let Err(e) = on_pcm(&buf[..n]) {
                    warn!("[timeshift] callback error: {}", e);
                    return Ok(());
                }
                
                // Zeit fortschreiben
                let frames = n as u64 / FRAME_BYTES;
                let ms = frames * 1000 / SAMPLE_RATE;
                cur_ts_ms += ms;
                
                // Kleine Pause um CPU zu schonen
                if ms < 10 {
                    sleep(Duration::from_millis(1));
                }
            }
        }
        
        // Zur nächsten Stunde
        retry_count = 0;
    }
}

/// Parst WAV-Header und gibt (data_start, file_size) zurück
fn parse_wav_header(file: &mut File) -> anyhow::Result<(u64, u64)> {
    file.seek(SeekFrom::Start(0))?;
    
    let mut riff = [0u8; 12];
    file.read_exact(&mut riff)?;
    
    // Check "RIFF" header
    if &riff[0..4] != b"RIFF" {
        anyhow::bail!("Not a RIFF file");
    }
    
    if &riff[8..12] != b"WAVE" {
        anyhow::bail!("Not a WAVE file");
    }
    
    // Chunks durchsuchen
    loop {
        let mut chunk_hdr = [0u8; 8];
        
        if file.read_exact(&mut chunk_hdr).is_err() {
            anyhow::bail!("Unexpected EOF while looking for 'data' chunk");
        }
        
        let id = &chunk_hdr[0..4];
        let len = u32::from_le_bytes(chunk_hdr[4..8].try_into()?) as u64;
        
        debug!("[timeshift] found chunk: {} ({} bytes)", 
               String::from_utf8_lossy(id), len);
        
        if id == b"fmt " {
            // fmt chunk lesen und validieren
            let mut fmt_buf = vec![0u8; len as usize];
            file.read_exact(&mut fmt_buf)?;
            
            let format = u16::from_le_bytes(fmt_buf[0..2].try_into()?);
            let channels = u16::from_le_bytes(fmt_buf[2..4].try_into()?);
            let sample_rate = u32::from_le_bytes(fmt_buf[4..8].try_into()?);
            let bits_per_sample = u16::from_le_bytes(fmt_buf[14..16].try_into()?);
            
            debug!("[timeshift] WAV format: format={}, channels={}, sample_rate={}, bits={}",
                   format, channels, sample_rate, bits_per_sample);
            
            if format != 1 { // PCM
                anyhow::bail!("Not PCM format (format={})", format);
            }
            if channels as u64 != CHANNELS {
                anyhow::bail!("Expected {} channels, got {}", CHANNELS, channels);
            }
            if sample_rate as u64 != SAMPLE_RATE {
                anyhow::bail!("Expected {} Hz, got {}", SAMPLE_RATE, sample_rate);
            }
            if bits_per_sample as u64 != BYTES_PER_SAMPLE * 8 {
                anyhow::bail!("Expected 16-bit, got {}-bit", bits_per_sample);
            }
            
            // Padding beachten (WAV chunks sind word-aligned)
            if len % 2 == 1 {
                let mut pad = [0u8; 1];
                file.read_exact(&mut pad)?;
            }
            
        } else if id == b"data" {
            // data chunk gefunden!
            let data_start = file.seek(SeekFrom::Current(0))?;
            let file_size = file.metadata()?.len();
            
            debug!("[timeshift] data chunk at byte {}, size {} bytes", data_start, len);
            
            // Wir bleiben HIER stehen - der nächste read() liefert Audio-Daten
            return Ok((data_start, file_size));
            
        } else {
            // Anderen Chunk überspringen
            debug!("[timeshift] skipping chunk '{}' ({} bytes)", 
                   String::from_utf8_lossy(id), len);
            
            let skip = if len % 2 == 1 { len + 1 } else { len };
            file.seek(SeekFrom::Current(skip as i64))?;
        }
    }
}
