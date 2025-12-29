use std::path::PathBuf;
use std::thread::sleep;
use std::time::Duration;

use hound::WavReader;
use log::{debug, info, warn};

/// Audio-Parameter (müssen zu Recorder passen)
const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: u16 = 2;

/// Blockierender Timeshift-Reader mit hound.
/// Ruft `on_pcm(bytes)` für jedes gelesene PCM-Chunk auf.
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
            if retry_count > 50 {
                // 50 * 200ms = 10s
                warn!("[timeshift] file not found after 10s: {:?}", wav_path);
                return Ok(()); // Leeres Stream-Ende
            }
            sleep(Duration::from_millis(200));
        }

        info!("[timeshift] opening {:?} with hound", wav_path);

        // WAV mit hound öffnen
        let mut reader = match WavReader::open(&wav_path) {
            Ok(r) => r,
            Err(e) => {
                warn!("[timeshift] hound open failed: {}", e);
                sleep(Duration::from_millis(1000));
                continue;
            }
        };

        let spec = reader.spec();
        debug!("[timeshift] WAV spec: {:?}", spec);

        // Validate format
        if spec.sample_rate != SAMPLE_RATE {
            warn!(
                "[timeshift] wrong sample rate: {} (expected {})",
                spec.sample_rate, SAMPLE_RATE
            );
            continue;
        }
        if spec.channels != CHANNELS {
            warn!(
                "[timeshift] wrong channels: {} (expected {})",
                spec.channels, CHANNELS
            );
            continue;
        }
        if spec.bits_per_sample != 16 {
            warn!(
                "[timeshift] wrong bits per sample: {} (expected 16)",
                spec.bits_per_sample
            );
            continue;
        }
        if spec.sample_format != hound::SampleFormat::Int {
            warn!("[timeshift] wrong sample format (expected Int16)");
            continue;
        }

        // Byte-Offset innerhalb dieser Stunde berechnen
        let offset_ms = cur_ts_ms.saturating_sub(hour_start_ms);
        let offset_frames = (offset_ms * SAMPLE_RATE as u64 / 1000) as u32;

        info!(
            "[timeshift] seeking to frame {} ({} ms offset)",
            offset_frames, offset_ms
        );

        // Seek mit hound (in Frames/Samples, nicht Bytes!)
        if let Err(e) = reader.seek(offset_frames) {
            warn!("[timeshift] hound seek failed: {}", e);
            return Err(anyhow::anyhow!(
                "timeshift seek failed at frame {}: {}",
                offset_frames,
                e
            ));
        }

        // Streaming-Loop für diese Stunde
        let mut bytes_read_total = 0;
        let mut chunk_count = 0;
        let mut samples_buf = vec![0i16; 4096]; // 2KB Samples = 4KB Bytes

        loop {
            // Samples mit hound lesen - KORRIGIERT für hound 3.5.1
            let mut samples_read = 0;
            {
                let mut samples_iter = reader.samples::<i16>();

                while samples_read < samples_buf.len() {
                    match samples_iter.next() {
                        Some(Ok(sample)) => {
                            samples_buf[samples_read] = sample;
                            samples_read += 1;
                        }
                        Some(Err(e)) => {
                            warn!("[timeshift] sample read error: {}", e);
                            break;
                        }
                        None => break,
                    }
                }
            }

            if samples_read == 0 {
                // EOF erreicht
                debug!("[timeshift] EOF for hour {}", hour);

                // Stundenwechsel erreicht?
                if cur_ts_ms >= hour_start_ms + 3600 * 1000 {
                    info!(
                        "[timeshift] hour {} completed ({} bytes total)",
                        hour, bytes_read_total
                    );
                    break; // nächste Stunde
                }

                // Sonst warten und von vorne lesen
                sleep(Duration::from_millis(100));

                // Zurück zum Anfang dieser Datei
                if let Err(e) = reader.seek(0) {
                    warn!("[timeshift] re-seek failed: {}", e);
                    break;
                }
                continue;
            }

            // i16 Samples zu u8 Bytes konvertieren
            let samples_slice = &samples_buf[..samples_read];
            let bytes = bytemuck::cast_slice::<i16, u8>(samples_slice);

            bytes_read_total += bytes.len();
            chunk_count += 1;

            // Debug: Ersten Chunk analysieren
            if chunk_count == 1 {
                debug!(
                    "[timeshift] first chunk: {} samples = {} bytes",
                    samples_read,
                    bytes.len()
                );
                if samples_read >= 4 {
                    debug!("[timeshift] first few samples: {:?}", &samples_slice[0..4]);
                }
            }

            // Nur alle 100 Chunks loggen
            if chunk_count % 100 == 0 {
                info!(
                    "[timeshift] read {} bytes total ({} chunks)",
                    bytes_read_total, chunk_count
                );
            }

            // PCM-Daten an Callback übergeben
            if let Err(e) = on_pcm(bytes) {
                warn!("[timeshift] callback error: {}", e);
                return Ok(());
            }

            // Zeit fortschreiben (in Frames, nicht Bytes!)
            let frames_read = samples_read as u64 / CHANNELS as u64;
            let ms_advanced = frames_read * 1000 / SAMPLE_RATE as u64;
            cur_ts_ms += ms_advanced;

            // Kleine Pause um CPU zu schonen
            if ms_advanced < 10 {
                sleep(Duration::from_millis(1));
            }
        }

        // Zur nächsten Stunde
        retry_count = 0;
    }
}

/// Hilfsfunktion: Prüft ob eine WAV-Datei das richtige Format hat
pub fn validate_wav_format(path: &PathBuf) -> anyhow::Result<()> {
    let reader = WavReader::open(path)?;
    let spec = reader.spec();

    if spec.sample_rate != SAMPLE_RATE {
        anyhow::bail!(
            "Wrong sample rate: {} (expected {})",
            spec.sample_rate,
            SAMPLE_RATE
        );
    }
    if spec.channels != CHANNELS {
        anyhow::bail!("Wrong channels: {} (expected {})", spec.channels, CHANNELS);
    }
    if spec.bits_per_sample != 16 {
        anyhow::bail!(
            "Wrong bits per sample: {} (expected 16)",
            spec.bits_per_sample
        );
    }

    info!("[timeshift] WAV validated: {:?}", path);
    Ok(())
}
