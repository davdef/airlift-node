// src/audio/http.rs

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use log::{debug, error, info, warn};
use tiny_http::{Header, Method, Response, Server, StatusCode};

use crate::audio::timeshift::stream_timeshift;
use crate::ring::{RingRead, RingReader};

// ============================================================================
// Public entry point
// ============================================================================

pub fn start_audio_http_server(
    bind: &str,
    wav_dir: PathBuf,
    ring_reader_factory: impl Fn() -> RingReader + Send + Sync + 'static,
) -> anyhow::Result<()> {
    let server = Server::http(bind).map_err(|e| anyhow::anyhow!(e))?;

    let wav_dir = Arc::new(wav_dir);
    let ring_factory = Arc::new(ring_reader_factory);

    info!("[audio] HTTP server on {}", bind);

    thread::spawn(move || {
        for req in server.incoming_requests() {
            if req.method() != &Method::Get {
                let _ = req.respond(Response::empty(StatusCode(405)));
                continue;
            }

            // ----------------------------------------------------------------
            // /audio/at?ts=...
            // ----------------------------------------------------------------
            if req.url().starts_with("/audio/at") {
                debug!("[audio] /audio/at requested: {}", req.url());
                handle_timeshift(req, wav_dir.clone());
                continue;
            }

            // ----------------------------------------------------------------
            // /audio/live
            // ----------------------------------------------------------------
            if req.url() == "/audio/live" {
                debug!("[audio] /audio/live requested");
                handle_live(req, ring_factory.clone());
                continue;
            }

            let _ = req.respond(Response::empty(StatusCode(404)));
        }
    });

    Ok(())
}

// ============================================================================
// Timeshift
// ============================================================================

fn handle_timeshift(req: tiny_http::Request, wav_dir: Arc<PathBuf>) {
    let ts = match extract_ts(req.url()) {
        Some(ts) => ts,
        None => {
            let _ =
                req.respond(Response::from_string("missing ts").with_status_code(StatusCode(400)));
            return;
        }
    };

    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let reader = ChannelReader { rx };

    let response = Response::new(
        StatusCode(200),
        vec![
            Header::from_bytes("Content-Type", "audio/mpeg").unwrap(),
            Header::from_bytes("Cache-Control", "no-store").unwrap(),
        ],
        reader,
        None,
        None,
    );

    if req.respond(response).is_err() {
        return;
    }

    thread::spawn(move || {
        // ffmpeg: PCM -> MP3
        let mut ffmpeg = match Command::new("ffmpeg")
            .args([
                "-loglevel",
                "error",
                "-f",
                "s16le",
                "-ar",
                "48000",
                "-ac",
                "2",
                "-i",
                "pipe:0",
                "-f",
                "mp3",
                "pipe:1",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
        {
            Ok(p) => p,
            Err(e) => {
                error!("[audio] ffmpeg spawn failed (timeshift): {}", e);
                return;
            }
        };

        let mut ff_stdin = ffmpeg.stdin.take().unwrap();
        let ff_stdout = ffmpeg.stdout.take().unwrap();

        // WAV follow-the-writer -> ffmpeg stdin
        let feeder = thread::spawn({
            let wav_dir = (*wav_dir).clone();
            move || {
                let res = stream_timeshift(wav_dir, ts, |pcm| {
                    ff_stdin.write_all(pcm)?;
                    Ok(())
                });
                if let Err(e) = res {
                    warn!("[audio] timeshift streaming ended: {}", e);
                }
            }
        });

        // ffmpeg stdout -> HTTP
        pump_ffmpeg_stdout(ff_stdout, tx);

        let _ = ffmpeg.kill();
        let _ = feeder.join();
        debug!("[audio] timeshift handler finished");
    });
}

// ============================================================================
// Live
// ============================================================================

fn handle_live(req: tiny_http::Request, ring_factory: Arc<dyn Fn() -> RingReader + Send + Sync>) {
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let reader = ChannelReader { rx };

    let response = Response::new(
        StatusCode(200),
        vec![
            Header::from_bytes("Content-Type", "audio/mpeg").unwrap(),
            Header::from_bytes("Cache-Control", "no-store").unwrap(),
        ],
        reader,
        None,
        None,
    );

    if req.respond(response).is_err() {
        return;
    }

    let mut ring_reader = ring_factory();

    thread::spawn(move || {
        let mut ffmpeg = match Command::new("ffmpeg")
            .args([
                "-loglevel",
                "error",
                "-f",
                "s16le",
                "-ar",
                "48000",
                "-ac",
                "2",
                "-i",
                "pipe:0",
                "-f",
                "mp3",
                "pipe:1",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
        {
            Ok(p) => p,
            Err(e) => {
                error!("[audio] ffmpeg spawn failed (live): {}", e);
                return;
            }
        };

        let mut ff_stdin = ffmpeg.stdin.take().unwrap();
        let ff_stdout = ffmpeg.stdout.take().unwrap();

        // Ring -> ffmpeg stdin
        let feeder = thread::spawn(move || {
            loop {
                match ring_reader.poll() {
                    RingRead::Chunk(slot) => {
                        let bytes = bytemuck::cast_slice::<i16, u8>(&slot.pcm);
                        if ff_stdin.write_all(bytes).is_err() {
                            warn!("[audio] ffmpeg stdin closed (live)");
                            break;
                        }
                    }
                    RingRead::Empty => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    RingRead::Gap { .. } => {}
                }
            }
        });

        // ffmpeg stdout -> HTTP
        pump_ffmpeg_stdout(ff_stdout, tx);

        let _ = ffmpeg.kill();
        let _ = feeder.join();
        debug!("[audio] live handler finished");
    });
}

// ============================================================================
// Helpers
// ============================================================================

fn pump_ffmpeg_stdout(mut ff_stdout: impl Read, tx: mpsc::Sender<Vec<u8>>) {
    let mut buf = [0u8; 8192];
    loop {
        match ff_stdout.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if tx.send(buf[..n].to_vec()).is_err() {
                    break; // client gone
                }
            }
            Err(_) => break,
        }
    }
}

struct ChannelReader {
    rx: mpsc::Receiver<Vec<u8>>,
}

impl Read for ChannelReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self.rx.recv() {
            Ok(chunk) => {
                let n = chunk.len().min(buf.len());
                buf[..n].copy_from_slice(&chunk[..n]);
                Ok(n)
            }
            Err(_) => Ok(0),
        }
    }
}

fn extract_ts(url: &str) -> Option<u64> {
    let q = url.split('?').nth(1)?;
    for part in q.split('&') {
        let mut it = part.split('=');
        if it.next()? == "ts" {
            return it.next()?.parse::<u64>().ok();
        }
    }
    None
}
