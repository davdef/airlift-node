// src/audio/http.rs

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

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
            info!("[audio] incoming {} {}", req.method(), req.url());

            if req.method() != &Method::Get {
                let _ = req.respond(Response::empty(StatusCode(405)));
                continue;
            }

            // ------------------------------------------------------------
            // /audio/at?ts=...
            // ------------------------------------------------------------
            if req.url().starts_with("/audio/at") {
                debug!("[audio] dispatching /audio/at");
                handle_timeshift(req, wav_dir.clone());
                continue;
            }

            // ------------------------------------------------------------
            // /audio/live
            // ------------------------------------------------------------
            if req.url().starts_with("/audio/live") {
                debug!("[audio] dispatching /audio/live");
                handle_live(req, ring_factory.clone());
                continue;
            }

            warn!("[audio] 404 for {}", req.url());
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
            warn!("[audio] /audio/at missing ts");
            let _ =
                req.respond(Response::from_string("missing ts").with_status_code(StatusCode(400)));
            return;
        }
    };

    info!("[audio] timeshift start ts={}", ts);

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

    // ===== ARBEITSTHREAD ZUERST STARTEN =====
    let worker_handle = thread::spawn({
        let tx = tx.clone();
        let wav_dir = wav_dir.clone();
        move || {
            info!("[audio] timeshift worker thread started for ts={}", ts);

            let mut ffmpeg = match spawn_ffmpeg("timeshift") {
                Some(p) => p,
                None => {
                    warn!("[audio] timeshift ffmpeg spawn failed");
                    return;
                }
            };

            let mut ff_stdin = ffmpeg.stdin.take().unwrap();
            let ff_stdout = ffmpeg.stdout.take().unwrap();

            let feeder = thread::spawn({
                let wav_dir = (*wav_dir).clone();
                move || {
                    let res = stream_timeshift(wav_dir, ts, |pcm| {
                        ff_stdin.write_all(pcm)?;
                        ff_stdin.flush()?;
                        Ok(())
                    });

                    match res {
                        Ok(_) => info!("[audio] timeshift feeder completed"),
                        Err(e) => warn!("[audio] timeshift feeder ended: {}", e),
                    }
                }
            });

            debug!("[audio] timeshift pumping ffmpeg stdout");
            pump_ffmpeg_stdout(ff_stdout, tx);

            let _ = ffmpeg.kill();
            let _ = feeder.join();
            info!("[audio] timeshift worker finished for ts={}", ts);
        }
    });

    // ===== ERST JETZT die HTTP-Response senden =====
    if req.respond(response).is_err() {
        warn!("[audio] timeshift client vanished early");
        return;
    }

    info!("[audio] timeshift HTTP response sent, streaming started");
    drop(worker_handle);
}

// ============================================================================
// Live - KORRIGIERTE VERSION
// ============================================================================

fn handle_live(req: tiny_http::Request, ring_factory: Arc<dyn Fn() -> RingReader + Send + Sync>) {
    info!("[audio] live start");

    // Kanal ERSTELLEN (vor Thread-Spawn)
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let reader = ChannelReader { rx };

    // Response vorbereiten
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

    // ===== ARBEITSTHREAD ZUERST STARTEN =====
    let worker_handle = thread::spawn({
        let tx = tx.clone(); // Sender für Thread klonen
        let ring_factory = ring_factory.clone();
        move || {
            info!("[audio] live worker thread started");

            let mut ring_reader = ring_factory();

            let head_seq = ring_reader.head_seq();
            let last_seq = ring_reader.last_seq();
            let fill = ring_reader.fill();
            info!(
                "[audio] live subscribe state head_seq={} last_seq={} fill={}",
                head_seq, last_seq, fill
            );

            info!("[audio] live waiting for first audio chunk…");

            let wait_start = Instant::now();
            let mut waits = 0u64;

            loop {
                match ring_reader.poll() {
                    RingRead::Chunk(_) => {
                        info!("[audio] live first audio chunk available");
                        break;
                    }
                    RingRead::Gap { missed } => {
                        warn!("[audio] live GAP while waiting missed={}", missed);
                    }
                    RingRead::Empty => {
                        waits += 1;

                        if waits % 200 == 0 {
                            let head_seq = ring_reader.head_seq();
                            let last_seq = ring_reader.last_seq();
                            let fill = ring_reader.fill();
                            info!(
                                "[audio] live still waiting (elapsed={}ms head_seq={} last_seq={} fill={} waits={})",
                                wait_start.elapsed().as_millis(),
                                head_seq,
                                last_seq,
                                fill,
                                waits,
                            );
                        }

                        thread::sleep(Duration::from_millis(5));
                    }
                }
            }

            // FFmpeg starten
            let mut ffmpeg = match spawn_ffmpeg("live") {
                Some(p) => p,
                None => return,
            };

            let mut ff_stdin = ffmpeg.stdin.take().unwrap();
            let ff_stdout = ffmpeg.stdout.take().unwrap();

            let feeder = thread::spawn(move || {
                let mut empty = 0u64;

                loop {
                    match ring_reader.poll() {
                        RingRead::Chunk(slot) => {
                            debug!(
                                "[audio] LIVE GOT CHUNK seq={} after {} empties",
                                slot.seq, empty
                            );
                            let bytes = bytemuck::cast_slice::<i16, u8>(&slot.pcm);
                            if ff_stdin.write_all(bytes).is_err() {
                                break;
                            }
                            if ff_stdin.flush().is_err() {
                                break;
                            }
                            empty = 0;
                        }
                        RingRead::Empty => {
                            empty += 1;
                            if empty % 1000 == 0 {
                                debug!("[audio] live still empty ({})", empty);
                            }
                            thread::sleep(Duration::from_millis(5));
                        }
                        RingRead::Gap { missed } => {
                            warn!("[audio] live GAP missed={}", missed);
                        }
                    }
                }
            });

            debug!("[audio] live pumping ffmpeg stdout");
            pump_ffmpeg_stdout(ff_stdout, tx);

            let _ = ffmpeg.kill();
            let _ = feeder.join();
            info!("[audio] live worker finished");
        }
    });

    // ===== ERST JETZT die HTTP-Response senden =====
    if req.respond(response).is_err() {
        warn!("[audio] live client vanished early");
        // Hier könntest du den Worker-Thread noch stoppen, falls nötig
        return;
    }

    info!("[audio] live HTTP response sent, streaming started");
    
    // Worker-Thread nicht joinen (blockiert), aber wir behalten den Handle
    drop(worker_handle); // Handle verwerfen, Thread läuft unabhängig weiter
}

// ============================================================================
// ffmpeg helper
// ============================================================================

fn spawn_ffmpeg(tag: &str) -> Option<std::process::Child> {
    debug!("[audio] spawning ffmpeg ({})", tag);

    match Command::new("ffmpeg")
        .args([
            "-loglevel",
            "error",
            // INPUT
            "-f",
            "s16le",
            "-ar",
            "48000",
            "-ac",
            "2",
            "-i",
            "pipe:0",
            // LOW LATENCY OUTPUT
            "-acodec",
            "libmp3lame",
            "-b:a",
            "128k",
            "-compression_level",
            "0",
            "-flush_packets",
            "1",
            "-fflags",
            "nobuffer",
            "-flags",
            "low_delay",
            "-max_delay",
            "0",
            "-muxdelay",
            "0",
            "-muxpreload",
            "0",
            "-f",
            "mp3",
            "pipe:1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
    {
        Ok(p) => Some(p),
        Err(e) => {
            error!("[audio] ffmpeg spawn failed ({}): {}", tag, e);
            None
        }
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn pump_ffmpeg_stdout(mut ff_stdout: impl Read, tx: mpsc::Sender<Vec<u8>>) {
    let mut buf = [0u8; 8192];
    let mut first = true;

    loop {
        match ff_stdout.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if first {
                    info!("[audio] ffmpeg produced first {} bytes", n);
                    first = false;
                }
                if tx.send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
            Err(e) => {
                warn!("[audio] ffmpeg stdout error: {}", e);
                break;
            }
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
