use std::collections::HashMap;
use std::io::Read;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc, Arc, Mutex,
};
use std::thread;

use log::{error, info, warn};
use tiny_http::{Header, Method, Response, Server, StatusCode};

use crate::audio::{EncodedFrameSource, EncodedRead};
use crate::codecs::registry::CodecRegistry;
use crate::codecs::{CodecInfo, ContainerKind, EncodedFrame};
use crate::core::error::{AudioError, AudioResult};

const MAX_STREAMS_TOTAL: usize = 32;
const MAX_STREAMS_PER_IP: usize = 4;

struct StreamRateLimiter {
    max_streams_total: usize,
    max_streams_per_ip: usize,
    active_total: AtomicUsize,
    active_per_ip: Mutex<HashMap<IpAddr, usize>>,
}

impl StreamRateLimiter {
    fn new(max_streams_total: usize, max_streams_per_ip: usize) -> Self {
        Self {
            max_streams_total,
            max_streams_per_ip,
            active_total: AtomicUsize::new(0),
            active_per_ip: Mutex::new(HashMap::new()),
        }
    }

    fn try_acquire(self: &Arc<Self>, remote: Option<SocketAddr>) -> Option<StreamPermit> {
        let ip = remote.map(|addr| addr.ip());

        loop {
            let current = self.active_total.load(Ordering::Relaxed);
            if current >= self.max_streams_total {
                return None;
            }
            if self
                .active_total
                .compare_exchange(current, current + 1, Ordering::SeqCst, Ordering::Relaxed)
                .is_ok()
            {
                break;
            }
        }

        if let Some(ip) = ip {
            let mut per_ip = self.active_per_ip.lock().unwrap();
            let count = per_ip.entry(ip).or_insert(0);
            if *count >= self.max_streams_per_ip {
                self.active_total.fetch_sub(1, Ordering::SeqCst);
                return None;
            }
            *count += 1;
        }

        Some(StreamPermit {
            limiter: Arc::clone(self),
            ip,
        })
    }

    fn release(&self, ip: Option<IpAddr>) {
        self.active_total.fetch_sub(1, Ordering::SeqCst);
        if let Some(ip) = ip {
            let mut per_ip = self.active_per_ip.lock().unwrap();
            if let Some(count) = per_ip.get_mut(&ip) {
                if *count > 1 {
                    *count -= 1;
                } else {
                    per_ip.remove(&ip);
                }
            }
        }
    }

}

struct StreamPermit {
    limiter: Arc<StreamRateLimiter>,
    ip: Option<IpAddr>,
}

impl Drop for StreamPermit {
    fn drop(&mut self) {
        self.limiter.release(self.ip);
    }
}

// ============================================================================
// Public entry
// ============================================================================

pub fn start_audio_http_server<F, R>(
    bind: &str,
    _wav_dir: PathBuf,
    ring_reader_factory: F,
    codec_id: Option<String>,
    codec_registry: Arc<CodecRegistry>,
) -> AudioResult<()>
where
    F: Fn() -> R + Send + Sync + 'static,
    R: EncodedFrameSource + Send + 'static,
{
    let server = Server::http(bind)
        .map_err(|e| AudioError::with_context("bind audio http server", e))?;
    let codec_id = require_codec_id(codec_id.as_deref())?;
    let codec_info = codec_registry
        .get_info(codec_id)
        .map_err(|e| AudioError::with_context("load codec info", e))?;
    validate_http_codec(codec_id, &codec_info)?;

    let ring_factory: Arc<dyn Fn() -> Box<dyn EncodedFrameSource + Send> + Send + Sync> =
        Arc::new(move || Box::new(ring_reader_factory()));
    let limiter = Arc::new(StreamRateLimiter::new(
        MAX_STREAMS_TOTAL,
        MAX_STREAMS_PER_IP,
    ));

    info!("[audio] HTTP server on {}", bind);

    thread::spawn(move || {
        for req in server.incoming_requests() {
            info!("[audio] incoming {} {}", req.method(), req.url());

            if req.method() != &Method::Get {
                let _ = req.respond(Response::empty(StatusCode(405)));
                continue;
            }

            if req.url().starts_with("/audio/at") {
                handle_timeshift(req);
                continue;
            }

            if req.url().starts_with("/audio/live") {
                handle_live_simple(req, ring_factory.clone(), limiter.clone());
                continue;
            }

            let _ = req.respond(Response::empty(StatusCode(404)));
        }
    });

    Ok(())
}

// ============================================================================
// Live Stream - Encoded Frames only
// ============================================================================

fn handle_live_simple(
    req: tiny_http::Request,
    ring_factory: Arc<dyn Fn() -> Box<dyn EncodedFrameSource + Send> + Send + Sync>,
    limiter: Arc<StreamRateLimiter>,
) {
    info!("[audio] live start (encoded frames)");

    let permit = match limiter.try_acquire(req.remote_addr()) {
        Some(permit) => permit,
        None => {
            warn!("[audio] live rejected (rate limit exceeded)");
            let _ = req.respond(Response::empty(StatusCode(429)));
            return;
        }
    };

    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let (stop_tx, stop_rx) = mpsc::channel::<()>();

    struct LiveReader {
        rx: mpsc::Receiver<Vec<u8>>,
        buffer: Vec<u8>,
        stop: Arc<AtomicBool>,
        _permit: StreamPermit,
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

            match self.rx.recv() {
                Ok(chunk) => {
                    let n = chunk.len().min(buf.len());
                    buf[..n].copy_from_slice(&chunk[..n]);
                    if n < chunk.len() {
                        self.buffer.extend_from_slice(&chunk[n..]);
                    }
                    Ok(n)
                }
                Err(mpsc::RecvError) => Ok(0),
            }
        }
    }

    let reader = LiveReader {
        rx,
        buffer: Vec::new(),
        stop: stop.clone(),
        _permit: permit,
    };

    let mut source = ring_factory();
    let source_notifier = source.notifier();
    let first_frame = match wait_for_frame(&mut *source) {
        Ok(frame) => frame,
        Err(e) => {
            error!("[audio] live failed to get first frame: {}", e);
            let _ = req.respond(Response::empty(StatusCode(503)));
            return;
        }
    };
    let content_type = match content_type_for_container(&first_frame.info) {
        Ok(content_type) => content_type,
        Err(e) => {
            error!("[audio] live unsupported container: {}", e);
            let _ = req.respond(Response::empty(StatusCode(415)));
            return;
        }
    };

    let response = Response::new(
        StatusCode(200),
        vec![
            Header::from_bytes("Content-Type", content_type).unwrap(),
            Header::from_bytes("Cache-Control", "no-store, no-cache").unwrap(),
            Header::from_bytes("Access-Control-Allow-Origin", "*").unwrap(),
        ],
        reader,
        None,
        None,
    );

    let feeder_stop = stop.clone();
    let initial_tx = tx.clone();
    let feeder_tx = tx;
    let feeder_stop_tx = stop_tx.clone();
    let feeder = thread::spawn(move || {
        let mut ring = source;
        info!("[audio] live feeder started");

        while !feeder_stop.load(Ordering::Relaxed) {
            let read = match ring.wait_for_read_or_stop(&feeder_stop) {
                Ok(Some(read)) => read,
                Ok(None) => break,
                Err(e) => {
                    warn!("[audio] live source error: {}", e);
                    break;
                }
            };

            match read {
                EncodedRead::Frame(frame) => {
                    if feeder_tx.send(frame.payload).is_err() {
                        feeder_stop.store(true, Ordering::Relaxed);
                        break;
                    }
                }
                EncodedRead::Gap { missed } => {
                    warn!("[audio] live GAP missed={}", missed);
                }
                EncodedRead::Empty => {}
            }
        }

        info!("[audio] live feeder exiting");
        let _ = feeder_stop_tx.send(());
    });

    thread::spawn({
        let source_notifier = source_notifier.clone();
        move || {
            let _ = stop_rx.recv();
            if let Some(notifier) = source_notifier {
                notifier.notify_all();
            }
            info!("[audio] live cleaning up");
            let _ = feeder.join();
        }
    });

    if initial_tx.send(first_frame.payload).is_err() {
        stop.store(true, Ordering::Relaxed);
        if let Some(notifier) = &source_notifier {
            notifier.notify_all();
        }
        let _ = stop_tx.send(());
        return;
    }

    if req.respond(response).is_err() {
        stop.store(true, Ordering::Relaxed);
        if let Some(notifier) = &source_notifier {
            notifier.notify_all();
        }
        let _ = stop_tx.send(());
    }
}

// ============================================================================
// Timeshift disabled (requires PCM/ffmpeg)
// ============================================================================

fn handle_timeshift(req: tiny_http::Request) {
    let _ = extract_ts(req.url());
    warn!("[audio] timeshift not available for encoded-only output");
    let _ = req.respond(Response::from_string("timeshift not available").with_status_code(501));
}

fn extract_ts(url: &str) -> Option<u64> {
    url.split('?').nth(1)?.split('&').find_map(|p| {
        let mut it = p.split('=');
        (it.next()? == "ts")
            .then(|| it.next()?.parse().ok())
            .flatten()
    })
}

fn content_type_for_container(info: &CodecInfo) -> AudioResult<&'static str> {
    match info.container {
        ContainerKind::Raw => Ok("application/octet-stream"),
        ContainerKind::Ogg => Ok("application/ogg"),
        ContainerKind::Mpeg => Ok("audio/mpeg"),
        ContainerKind::Rtp => Ok("application/rtp"),
    }
}

fn wait_for_frame(source: &mut dyn EncodedFrameSource) -> AudioResult<EncodedFrame> {
    loop {
        match source
            .wait_for_read()
            .map_err(|e| AudioError::with_context("wait for encoded frame", e))?
        {
            EncodedRead::Frame(frame) => return Ok(frame),
            EncodedRead::Gap { missed } => {
                warn!("[audio] live gap while waiting for first frame: {}", missed);
            }
            EncodedRead::Empty => {}
        }
    }
}

fn require_codec_id(codec_id: Option<&str>) -> AudioResult<&str> {
    codec_id.ok_or_else(|| AudioError::message("missing codec_id for audio http output"))
}

fn validate_http_codec(codec_id: &str, info: &CodecInfo) -> AudioResult<()> {
    match info.container {
        ContainerKind::Raw | ContainerKind::Ogg | ContainerKind::Mpeg => Ok(()),
        ContainerKind::Rtp => Err(AudioError::message(format!(
            "audio http output does not accept RTP container (codec_id '{}', container {:?})",
            codec_id, info.container
        ))),
    }
}
