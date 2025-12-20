// src/web/audio_api.rs

use axum::{
    extract::{Query, State},
    response::{Response},
    http::{header, StatusCode},
    body::Body,
};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;

use crate::audio::timeshift::stream_timeshift;
use crate::ring::{RingRead, RingReader};

#[derive(Deserialize)]
pub struct AudioTimestampQuery {
    pub ts: Option<u64>,
}

// State für Audio-Endpunkte
#[derive(Clone)]
pub struct AudioState {
    pub wav_dir: PathBuf,
    pub ring: Arc<crate::ring::AudioRing>,
}

// Live-Audio Stream aus Ringbuffer
pub async fn live_audio_stream(
    State(state): State<AudioState>,
) -> Response<Body> {
    use std::process::{Command, Stdio};
    use std::io::{Read, Write};
    
    let mut ring_reader = state.ring.subscribe();
    
    // FFmpeg starten
    let mut ffmpeg = match Command::new("ffmpeg")
        .args([
            "-loglevel", "error",
            "-f", "s16le",
            "-ar", "48000",
            "-ac", "2",
            "-i", "pipe:0",
            "-acodec", "libmp3lame",
            "-b:a", "128k",
            "-compression_level", "0",
            "-flush_packets", "1",
            "-fflags", "nobuffer",
            "-flags", "low_delay",
            "-max_delay", "0",
            "-muxdelay", "0",
            "-muxpreload", "0",
            "-f", "mp3",
            "pipe:1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn() {
        Ok(p) => p,
        Err(e) => {
            log::error!("[audio] ffmpeg spawn failed: {}", e);
            return Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::empty())
                .unwrap();
        }
    };
    
    let ff_stdin = ffmpeg.stdin.take().unwrap();
    let mut ff_stdout = ffmpeg.stdout.take().unwrap();
    
    // PCM-Feeder in separatem Thread
    let feeder_handle = std::thread::spawn(move || {
        let mut ff_stdin = ff_stdin;
        
        loop {
            match ring_reader.poll() {
                RingRead::Chunk(slot) => {
                    let bytes = bytemuck::cast_slice::<i16, u8>(&slot.pcm);
                    if ff_stdin.write_all(bytes).is_err() {
                        break;
                    }
                    if ff_stdin.flush().is_err() {
                        break;
                    }
                }
                RingRead::Empty => {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                RingRead::Gap { missed } => {
                    log::warn!("[audio] live gap missed={}", missed);
                }
            }
        }
    });
    
    // Lese FFmpeg-Ausgabe
    let mut buffer = Vec::new();
    if let Err(e) = ff_stdout.read_to_end(&mut buffer) {
        log::error!("[audio] failed to read ffmpeg output: {}", e);
    }
    
    // Warte auf Feeder-Thread
    let _ = feeder_handle.join();
    let _ = ffmpeg.kill();
    
    Response::builder()
        .header(header::CONTENT_TYPE, "audio/mpeg")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(buffer))
        .unwrap()
}

// Historischer Audio-Stream aus WAV-Dateien
pub async fn historical_audio_stream(
    State(state): State<AudioState>,
    Query(params): Query<AudioTimestampQuery>,
) -> Response<Body> {
    let start_ts_ms = match params.ts {
        Some(ts) => ts,
        None => {
            return Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Body::from("Missing 'ts' parameter"))
                .unwrap();
        }
    };
    
    use std::process::{Command, Stdio};
    use std::io::{Read, Write};
    
    // FFmpeg starten
    let mut ffmpeg = match Command::new("ffmpeg")
        .args([
            "-loglevel", "error",
            "-f", "s16le",
            "-ar", "48000",
            "-ac", "2",
            "-i", "pipe:0",
            "-acodec", "libmp3lame",
            "-b:a", "128k",
            "-compression_level", "0",
            "-flush_packets", "1",
            "-fflags", "nobuffer",
            "-flags", "low_delay",
            "-max_delay", "0",
            "-muxdelay", "0",
            "-muxpreload", "0",
            "-f", "mp3",
            "pipe:1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn() {
        Ok(p) => p,
        Err(e) => {
            log::error!("[audio] ffmpeg spawn failed: {}", e);
            return Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::empty())
                .unwrap();
        }
    };
    
    let ff_stdin = ffmpeg.stdin.take().unwrap();
    let mut ff_stdout = ffmpeg.stdout.take().unwrap();
    
    // Timeshift-Feeder in separatem Thread
    let feeder_handle = std::thread::spawn({
        let wav_dir = state.wav_dir.clone();
        move || {
            let mut ff_stdin = ff_stdin;
            let result = stream_timeshift(wav_dir, start_ts_ms, |pcm| {
                ff_stdin.write_all(pcm)?;
                ff_stdin.flush()?;
                Ok(())
            });
            
            match result {
                Ok(_) => log::info!("[audio] timeshift feeder completed"),
                Err(e) => log::warn!("[audio] timeshift feeder error: {}", e),
            }
        }
    });
    
    // Lese FFmpeg-Ausgabe
    let mut buffer = Vec::new();
    if let Err(e) = ff_stdout.read_to_end(&mut buffer) {
        log::error!("[audio] failed to read ffmpeg output: {}", e);
    }
    
    // Warte auf Feeder-Thread
    let _ = feeder_handle.join();
    let _ = ffmpeg.kill();
    
    Response::builder()
        .header(header::CONTENT_TYPE, "audio/mpeg")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(buffer))
        .unwrap()
}
