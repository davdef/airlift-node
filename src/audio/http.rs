// src/audio/http.rs

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Arc;
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

    let mut ffmpeg = match spawn_ffmpeg("timeshift") {
        Some(p) => p,
        None => {
            warn!("[audio] timeshift ffmpeg spawn failed");
            let _ = req.respond(Response::empty(StatusCode(500)));
            return;
        }
    };
    let ff_stdin = ffmpeg.stdin.take().unwrap();
    let ff_stdout = ffmpeg.stdout.take().unwrap();

    let response = Response::new(
        StatusCode(200),
        vec![
            Header::from_bytes("Content-Type", "audio/mpeg").unwrap(),
            Header::from_bytes("Cache-Control", "no-store").unwrap(),
        ],
        FfmpegBody::new(
            ffmpeg,
            ff_stdout,
            spawn_timeshift_feeder(wav_dir.clone(), ts, ff_stdin),
        ),
        None,
        None,
    );

    // ===== ARBEITSTHREAD ZUERST STARTEN =====
    if req.respond(response).is_err() {
        warn!("[audio] timeshift client vanished early");
        return;
    }

    info!("[audio] timeshift HTTP response sent, streaming started");
}

// ============================================================================
// Live - KORRIGIERTE VERSION
// ============================================================================

fn handle_live(req: tiny_http::Request, ring_factory: Arc<dyn Fn() -> RingReader + Send + Sync>) {
    info!("[audio] live start");

    let mut ffmpeg = match spawn_ffmpeg("live") {
        Some(p) => p,
        None => {
            warn!("[audio] live ffmpeg spawn failed");
            let _ = req.respond(Response::empty(StatusCode(500)));
            return;
        }
    };
    let ff_stdin = ffmpeg.stdin.take().unwrap();
    let ff_stdout = ffmpeg.stdout.take().unwrap();

    // Response vorbereiten
    let response = Response::new(
        StatusCode(200),
        vec![
            Header::from_bytes("Content-Type", "audio/mpeg").unwrap(),
            Header::from_bytes("Cache-Control", "no-store").unwrap(),
        ],
        FfmpegBody::new(
            ffmpeg,
            ff_stdout,
            spawn_live_feeder(ring_factory.clone(), ff_stdin),
        ),
        None,
        None,
    );

    if req.respond(response).is_err() {
        warn!("[audio] live client vanished early");
        return;
    }

    info!("[audio] live HTTP response sent, streaming started");
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

fn spawn_timeshift_feeder(
    wav_dir: Arc<PathBuf>,
    ts: u64,
    mut ff_stdin: ChildStdin,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        info!("[audio] timeshift worker thread started for ts={}", ts);

        let res = stream_timeshift((*wav_dir).clone(), ts, |pcm| {
            ff_stdin.write_all(pcm)?;
            ff_stdin.flush()?;
            Ok(())
        });

        match res {
            Ok(_) => info!("[audio] timeshift feeder completed"),
            Err(e) => warn!("[audio] timeshift feeder ended: {}", e),
        }
    })
}

fn spawn_live_feeder(
    ring_factory: Arc<dyn Fn() -> RingReader + Send + Sync>,
    mut ff_stdin: ChildStdin,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
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

        info!("[audio] live feeder ended");
    })
}

struct FfmpegBody {
    stdout: ChildStdout,
    child: Option<Child>,
    feeder: Option<thread::JoinHandle<()>>,
}

impl FfmpegBody {
    fn new(child: Child, stdout: ChildStdout, feeder: thread::JoinHandle<()>) -> Self {
        Self {
            stdout,
            child: Some(child),
            feeder: Some(feeder),
        }
    }
}

impl Read for FfmpegBody {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.stdout.read(buf)
    }
}

impl Drop for FfmpegBody {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
        }

        if let Some(handle) = self.feeder.take() {
            let _ = handle.join();
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
