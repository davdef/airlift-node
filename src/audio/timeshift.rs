// src/audio/timeshift.rs

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::thread::sleep;
use std::time::Duration;

use hound::WavReader;

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

    let mut cur_ts_ms = start_ts_ms;

    loop {
        // aktuelle Stunde bestimmen
        let hour = (cur_ts_ms / 1000) / 3600;
        let hour_start_ms = hour * 3600 * 1000;

        let wav_path = wav_dir.join(format!("{}.wav", hour));

        // warten, bis Datei existiert
        while !wav_path.exists() {
            sleep(Duration::from_millis(200));
        }

        // WAV öffnen
        let mut reader = WavReader::open(&wav_path)?;
        let spec = reader.spec();

        // sanity check (bewusst hart)
        if spec.sample_rate as u64 != SAMPLE_RATE || spec.channels as u64 != CHANNELS {
            anyhow::bail!("unexpected WAV format in {:?}", wav_path);
        }

        // Byte-Offset berechnen
        let offset_ms = cur_ts_ms.saturating_sub(hour_start_ms);
        let offset_frames = offset_ms * SAMPLE_RATE / 1000;
        let offset_bytes = offset_frames * FRAME_BYTES;

        // auf PCM-Daten springen
        let pcm_start = reader.into_inner().into_inner();
        let mut file = pcm_start;
        let data_start = wav_data_start(&mut file)?;

// Offset berechnen
let offset_ms = cur_ts_ms.saturating_sub(hour_start_ms);
let offset_frames = offset_ms * SAMPLE_RATE / 1000;
let offset_bytes = offset_frames * FRAME_BYTES;

// Start der PCM-Daten
let data_start = wav_data_start(&mut file)?;

// Clamp: nicht hinter Dateiende springen
let file_len = file.metadata()?.len();
let wanted = data_start + offset_bytes;

let start_pos = if wanted >= file_len {
    data_start
} else {
    wanted
};

file.seek(SeekFrom::Start(start_pos))?;

        // Streaming-Loop für diese Stunde
        loop {
            let mut buf = [0u8; 8192];
            let n = file.read(&mut buf)?;

            if n > 0 {
                on_pcm(&buf[..n])?;

                // Zeit fortschreiben
                let frames = n as u64 / FRAME_BYTES;
                let ms = frames * 1000 / SAMPLE_RATE;
                cur_ts_ms += ms;
            } else {
                // EOF → warten, dann erneut lesen
                sleep(Duration::from_millis(100));

                // Stundenwechsel erreicht?
                if cur_ts_ms >= hour_start_ms + 3600 * 1000 {
                    break; // nächste Stunde öffnen
                }
            }
        }
    }
}

/// Ermittelt Start der PCM-Daten im WAV.
/// (hound abstrahiert das leider nicht sauber)
fn wav_data_start(file: &mut File) -> anyhow::Result<u64> {
    file.seek(SeekFrom::Start(0))?;

    let mut header = [0u8; 12];
    file.read_exact(&mut header)?;

    loop {
        let mut chunk_hdr = [0u8; 8];
        file.read_exact(&mut chunk_hdr)?;

        let id = &chunk_hdr[0..4];
        let len = u32::from_le_bytes(chunk_hdr[4..8].try_into()?);

        if id == b"data" {
            let pos = file.seek(SeekFrom::Current(0))?;
            return Ok(pos);
        }

        // skip chunk (+ padding)
        let skip = (len + 1) & !1;
        file.seek(SeekFrom::Current(skip as i64))?;
    }
}
