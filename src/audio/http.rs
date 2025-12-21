// src/audio/http.rs — Vollständig korrigierte Version
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::thread;
use std::time::{Duration, Instant};

use log::{error, info, warn};
use tiny_http::{Header, Method, Response, Server, StatusCode};

use crate::audio::timeshift::stream_timeshift;
use crate::ring::{RingRead, RingReader};

// ============================================================================
// Public entry
// ============================================================================

pub fn start_audio_http_server(
    bind: &str,
    wav_dir: PathBuf,
    ring_reader_factory: impl Fn() -> RingReader + Send + Sync + 'static,
) -> anyhow::Result<()> {
    let server = Server::http(bind)
        .map_err(|e| anyhow::anyhow!(e))?;

    let wav_dir = Arc::new(wav_dir);
    let ring_factory: Arc<dyn Fn() -> RingReader + Send + Sync> = Arc::new(ring_reader_factory);

    info!("[audio] HTTP server on {}", bind);

    thread::spawn(move || {
        for req in server.incoming_requests() {
            info!("[audio] incoming {} {}", req.method(), req.url());

            if req.method() != &Method::Get {
                let _ = req.respond(Response::empty(StatusCode(405)));
                continue;
            }

            if req.url().starts_with("/audio/at") {
                handle_timeshift(req, wav_dir.clone());
                continue;
            }

            if req.url().starts_with("/audio/live") {
                handle_live_simple(req, ring_factory.clone());
                continue;
            }

            let _ = req.respond(Response::empty(StatusCode(404)));
        }
    });

    Ok(())
}

// ============================================================================
// Live Stream - Direkte Ogg/Opus ohne ffmpeg
// ============================================================================

fn handle_live_simple(req: tiny_http::Request, ring_factory: Arc<dyn Fn() -> RingReader + Send + Sync>) {
    use std::io::ErrorKind;
    
    info!("[audio] live start (ffmpeg fallback)");

    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    
    struct LiveReader {
        rx: mpsc::Receiver<Vec<u8>>,
        buffer: Vec<u8>,
        stop: Arc<AtomicBool>,
    }
    
    impl Read for LiveReader {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if !self.buffer.is_empty() {
                let n = self.buffer.len().min(buf.len());
                buf[..n].copy_from_slice(&self.buffer[..n]);
                self.buffer.drain(..n);
                return Ok(n);
            }
            
            if self.stop.load(Ordering::Relaxed) {
                return Ok(0);
            }
            
            match self.rx.recv_timeout(Duration::from_millis(100)) {
                Ok(chunk) => {
                    let n = chunk.len().min(buf.len());
                    buf[..n].copy_from_slice(&chunk[..n]);
                    if n < chunk.len() {
                        self.buffer.extend_from_slice(&chunk[n..]);
                    }
                    Ok(n)
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    Err(std::io::ErrorKind::WouldBlock.into())
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => Ok(0),
            }
        }
    }
    
    let reader = LiveReader {
        rx,
        buffer: Vec::new(),
        stop: stop.clone(),
    };

    let response = Response::new(
        StatusCode(200),
        vec![
            Header::from_bytes("Content-Type", "audio/ogg; codecs=opus").unwrap(),
            Header::from_bytes("Cache-Control", "no-store, no-cache").unwrap(),
            Header::from_bytes("Access-Control-Allow-Origin", "*").unwrap(),
        ],
        reader,
        None,
        None,
    );

    thread::spawn({
        let stop = stop.clone();
        move || {
            info!("[audio] live: starting ffmpeg");
            
            let mut ffmpeg = match Command::new("ffmpeg")
                .args([
                    "-loglevel", "error",
                    "-f", "s16le",
                    "-ar", "48000",
                    "-ac", "2",
                    "-i", "pipe:0",
                    "-c:a", "libopus",
                    "-application", "audio",
                    "-frame_duration", "20",
                    "-vbr", "off",
                    "-b:a", "128k",
                    "-f", "ogg",
                    "pipe:1",
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
            {
                Ok(p) => {
                    info!("[audio] live ffmpeg started");
                    p
                }
                Err(e) => {
                    error!("[audio] live failed to spawn ffmpeg: {}", e);
                    stop.store(true, Ordering::Relaxed);
                    return;
                }
            };

            let mut ff_stdin = ffmpeg.stdin.take().unwrap();
            let mut ff_stdout = ffmpeg.stdout.take().unwrap();

            // Feeder thread
            let feeder_stop = stop.clone();
            let feeder = thread::spawn(move || {
                let mut ring = ring_factory();
                let mut silence_counter = 0;
                info!("[audio] live feeder started");
                
                while !feeder_stop.load(Ordering::Relaxed) {
                    match ring.poll() {
                        RingRead::Chunk(slot) => {
                            let bytes = bytemuck::cast_slice::<i16, u8>(&slot.pcm);
                            if ff_stdin.write_all(bytes).is_err() {
                                break;
                            }
                            silence_counter = 0;
                        }
                        RingRead::Empty => {
                            silence_counter += 1;
                            
                            // Nach 10 leeren Zyklen (100ms) Stille senden
                            if silence_counter > 10 {
                                let silence = vec![0i16; 4800 * 2]; // 100ms Stille
                                let bytes = bytemuck::cast_slice::<i16, u8>(&silence);
                                let _ = ff_stdin.write_all(bytes);
                                silence_counter = 0;
                            }
                            
                            thread::sleep(Duration::from_millis(10));
                        }
                        RingRead::Gap { missed } => {
                            warn!("[audio] live GAP missed={}", missed);
                            // Stille senden für Lücke
                            let silence = vec![0i16; 4800 * 2]; // 100ms Stille
                            let bytes = bytemuck::cast_slice::<i16, u8>(&silence);
                            let _ = ff_stdin.write_all(bytes);
                        }
                    }
                }
                
                info!("[audio] live feeder exiting");
                drop(ff_stdin);
            });

            // Pumper thread
            let pumper_stop = stop.clone();
            let pumper = thread::spawn(move || {
                let mut buffer = [0u8; 8192];
                info!("[audio] live pumper started");
                
                while !pumper_stop.load(Ordering::Relaxed) {
                    match ff_stdout.read(&mut buffer) {
                        Ok(0) => {
                            info!("[audio] live ffmpeg EOF");
                            break;
                        }
                        Ok(n) => {
                            if tx.send(buffer[..n].to_vec()).is_err() {
                                info!("[audio] live client disconnected");
                                break;
                            }
                        }
                        Err(e) if e.kind() == ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(10));
                            continue;
                        }
                        Err(e) => {
                            info!("[audio] live read error: {}", e);
                            break;
                        }
                    }
                }
                
                pumper_stop.store(true, Ordering::Relaxed);
                info!("[audio] live pumper exiting");
            });

            while !stop.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(100));
            }

            info!("[audio] live cleaning up");
            let _ = ffmpeg.kill();
            let _ = ffmpeg.wait();
            let _ = feeder.join();
            let _ = pumper.join();
        }
    });

    if req.respond(response).is_err() {
        stop.store(true, Ordering::Relaxed);
    }
}

// ============================================================================
// Ogg/Opus Encoder (same as icecast_out.rs)
// ============================================================================

struct OggOpusEncoder {
    enc: opus::Encoder,
    pw: ogg::writing::PacketWriter<Vec<u8>>,
    serial: u32,
    gp: u64,
}

impl OggOpusEncoder {
    fn new(bitrate: i32) -> anyhow::Result<Self> {
        use opus::{Application, Channels, Encoder};
        
        let mut enc = Encoder::new(48_000, Channels::Stereo, Application::Audio)
            .map_err(|e| anyhow::anyhow!("Opus encoder error: {}", e))?;
        
        enc.set_bitrate(opus::Bitrate::Bits(bitrate))
            .map_err(|e| anyhow::anyhow!("Opus bitrate error: {}", e))?;

        let serial = rand::random::<u32>();
        let mut pw = ogg::writing::PacketWriter::new(Vec::with_capacity(64 * 1024));

        // Write Ogg/Opus headers immediately (crucial for browser!)
        pw.write_packet(
            opus_head().into(),
            serial,
            ogg::PacketWriteEndInfo::EndPage,
            0,
        )?;
        
        pw.write_packet(
            opus_tags("airlift-http").into(),
            serial,
            ogg::PacketWriteEndInfo::EndPage,
            0,
        )?;

        Ok(Self {
            enc,
            pw,
            serial,
            gp: 0,
        })
    }

    fn encode_100ms(&mut self, pcm: &[i16]) -> anyhow::Result<Vec<u8>> {
        const OPUS_FRAME_SAMPLES_PER_CH: usize = 960; // 20 ms @ 48k
        const CHANNELS: usize = 2;
        const OPUS_FRAME_I16: usize = OPUS_FRAME_SAMPLES_PER_CH * CHANNELS;

        if pcm.len() % OPUS_FRAME_I16 != 0 {
            return Err(anyhow::anyhow!(
                "PCM len {} not multiple of {} (20ms stereo frame)",
                pcm.len(),
                OPUS_FRAME_I16
            ));
        }

        let mut opus_buf = [0u8; 4000];
        let mut frames = pcm.chunks_exact(OPUS_FRAME_I16);
        let n_frames = frames.len();
        
        if n_frames == 0 {
            return Ok(Vec::new());
        }
        
        let last = n_frames - 1;

        for (i, frame) in frames.by_ref().enumerate() {
            let n = self.enc.encode(frame, &mut opus_buf)
                .map_err(|e| anyhow::anyhow!("Opus encode error: {}", e))?;

            self.gp += OPUS_FRAME_SAMPLES_PER_CH as u64;

            let end = if i == last {
                ogg::PacketWriteEndInfo::EndPage
            } else {
                ogg::PacketWriteEndInfo::NormalPacket
            };

            self.pw.write_packet(
                opus_buf[..n].to_vec().into_boxed_slice(),
                self.serial,
                end,
                self.gp,
            )?;
        }

        // Get encoded data without resetting writer
        let mut out = Vec::new();
        std::mem::swap(&mut out, self.pw.inner_mut());
        Ok(out)
    }
}

fn opus_head() -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(b"OpusHead");
    p.push(1); // version
    p.push(2); // channels
    p.extend_from_slice(&312u16.to_le_bytes()); // pre-skip
    p.extend_from_slice(&48_000u32.to_le_bytes());
    p.extend_from_slice(&0i16.to_le_bytes()); // gain
    p.push(0); // mapping family 0 (stereo)
    p
}

fn opus_tags(vendor: &str) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(b"OpusTags");
    p.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    p.extend_from_slice(vendor.as_bytes());
    p.extend_from_slice(&0u32.to_le_bytes()); // no comments
    p
}

// ============================================================================
// Timeshift (using ffmpeg) - KORRIGIERT für stream_timeshift
// ============================================================================

fn handle_timeshift(req: tiny_http::Request, wav_dir: Arc<PathBuf>) {
    use std::io::ErrorKind;
    
    let ts = match extract_ts(req.url()) {
        Some(ts) => ts,
        None => {
            let _ = req.respond(Response::from_string("missing ts").with_status_code(400));
            return;
        }
    };

    info!("[audio] timeshift start ts={}", ts);

    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    
    struct TimeshiftReader {
        rx: mpsc::Receiver<Vec<u8>>,
        buffer: Vec<u8>,
        stop: Arc<AtomicBool>,
    }
    
    impl Read for TimeshiftReader {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if !self.buffer.is_empty() {
                let n = self.buffer.len().min(buf.len());
                buf[..n].copy_from_slice(&self.buffer[..n]);
                self.buffer.drain(..n);
                return Ok(n);
            }
            
            if self.stop.load(Ordering::Relaxed) {
                return Ok(0);
            }
            
            match self.rx.recv_timeout(Duration::from_millis(500)) {
                Ok(chunk) => {
                    let n = chunk.len().min(buf.len());
                    buf[..n].copy_from_slice(&chunk[..n]);
                    if n < chunk.len() {
                        self.buffer.extend_from_slice(&chunk[n..]);
                    }
                    Ok(n)
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    Err(std::io::ErrorKind::WouldBlock.into())
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => Ok(0),
            }
        }
    }
    
    let reader = TimeshiftReader {
        rx,
        buffer: Vec::new(),
        stop: stop.clone(),
    };

    let response = Response::new(
        StatusCode(200),
        vec![
            Header::from_bytes("Content-Type", "audio/ogg; codecs=opus").unwrap(),
            Header::from_bytes("Cache-Control", "no-store, no-cache").unwrap(),
            Header::from_bytes("Access-Control-Allow-Origin", "*").unwrap(),
        ],
        reader,
        None,
        None,
    );

    thread::spawn({
        let stop = stop.clone();
        let wav_dir = (*wav_dir).clone();
        move || {
            info!("[audio] timeshift: starting ffmpeg for timestamp {}", ts);
            
            let mut ffmpeg = match Command::new("ffmpeg")
                .args([
                    "-loglevel", "error",
                    "-f", "s16le",       // RAW PCM input format
                    "-ar", "48000",      // Sample rate
                    "-ac", "2",          // Stereo
                    "-i", "pipe:0",      // Input from stdin
                    "-c:a", "libopus",
                    "-application", "audio",
                    "-frame_duration", "20",
                    "-vbr", "off",
                    "-b:a", "128k",
                    "-f", "ogg",
                    "pipe:1",
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
            {
                Ok(p) => {
                    info!("[audio] timeshift ffmpeg started");
                    p
                }
                Err(e) => {
                    error!("[audio] timeshift failed to spawn ffmpeg: {}", e);
                    stop.store(true, Ordering::Relaxed);
                    return;
                }
            };

            let mut ff_stdin = ffmpeg.stdin.take().unwrap();
            let mut ff_stdout = ffmpeg.stdout.take().unwrap();

            // Thread to feed WAV data to ffmpeg
            let feeder_stop = stop.clone();
            let feeder = thread::spawn({
                let wav_dir = wav_dir.clone();
                move || {
                    info!("[audio] timeshift feeder starting for ts={}", ts);
                    
                    // stream_timeshift gibt bereits &[u8] zurück (RAW PCM Bytes)
                    let result = stream_timeshift(wav_dir, ts, |pcm_bytes| {
                        if feeder_stop.load(Ordering::Relaxed) {
                            return Ok(());
                        }
                        
                        // pcm_bytes ist bereits &[u8] (RAW PCM in s16le format)
                        if ff_stdin.write_all(pcm_bytes).is_err() {
                            return Err(anyhow::anyhow!("Pipe broken"));
                        }
                        Ok(())
                    });
                    
                    if let Err(e) = result {
                        error!("[audio] timeshift stream error: {}", e);
                    }
                    
                    drop(ff_stdin);
                    info!("[audio] timeshift feeder exiting");
                }
            });

            // Thread to read ffmpeg output and send to HTTP client
            let pumper_stop = stop.clone();
            let pumper = thread::spawn(move || {
                let mut buffer = [0u8; 8192];
                info!("[audio] timeshift pumper started");
                
                while !pumper_stop.load(Ordering::Relaxed) {
                    match ff_stdout.read(&mut buffer) {
                        Ok(0) => {
                            info!("[audio] timeshift ffmpeg EOF");
                            break;
                        }
                        Ok(n) => {
                            if tx.send(buffer[..n].to_vec()).is_err() {
                                info!("[audio] timeshift client disconnected");
                                break;
                            }
                        }
                        Err(e) if e.kind() == ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(10));
                            continue;
                        }
                        Err(e) => {
                            info!("[audio] timeshift read error: {}", e);
                            break;
                        }
                    }
                }
                
                pumper_stop.store(true, Ordering::Relaxed);
                info!("[audio] timeshift pumper exiting");
            });

            // Wait for stop signal or threads to finish
            while !stop.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(100));
            }

            info!("[audio] timeshift cleaning up");
            let _ = ffmpeg.kill();
            let _ = ffmpeg.wait();
            let _ = feeder.join();
            let _ = pumper.join();
        }
    });

    if req.respond(response).is_err() {
        stop.store(true, Ordering::Relaxed);
    }
}

fn extract_ts(url: &str) -> Option<u64> {
    url.split('?')
        .nth(1)?
        .split('&')
        .find_map(|p| {
            let mut it = p.split('=');
            (it.next()? == "ts").then(|| it.next()?.parse().ok()).flatten()
        })
}
