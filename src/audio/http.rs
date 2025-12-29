use std::io::Read;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::thread;

use log::{error, info, warn};
use tiny_http::{Header, Method, Response, Server, StatusCode};

use crate::audio::{EncodedFrameSource, EncodedRead};
use crate::codecs::registry::CodecRegistry;
use crate::codecs::{CodecInfo, ContainerKind, EncodedFrame};

// ============================================================================
// Public entry
// ============================================================================

pub fn start_audio_http_server<F, R>(
    bind: &str,
    _wav_dir: PathBuf,
    ring_reader_factory: F,
    codec_id: Option<String>,
    codec_registry: Arc<CodecRegistry>,
) -> anyhow::Result<()>
where
    F: Fn() -> R + Send + Sync + 'static,
    R: EncodedFrameSource + Send + 'static,
{
    let server = Server::http(bind).map_err(|e| anyhow::anyhow!(e))?;
    let codec_id = require_codec_id(codec_id.as_deref())?;
    let codec_info = codec_registry.get_info(codec_id)?;
    validate_http_codec(codec_id, &codec_info)?;

    let ring_factory: Arc<dyn Fn() -> Box<dyn EncodedFrameSource + Send> + Send + Sync> =
        Arc::new(move || Box::new(ring_reader_factory()));

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
                handle_live_simple(req, ring_factory.clone());
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
) {
    info!("[audio] live start (encoded frames)");

    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let (stop_tx, stop_rx) = mpsc::channel::<()>();

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

fn content_type_for_container(info: &CodecInfo) -> anyhow::Result<&'static str> {
    match info.container {
        ContainerKind::Raw => Ok("application/octet-stream"),
        ContainerKind::Ogg => Ok("application/ogg"),
        ContainerKind::Mpeg => Ok("audio/mpeg"),
        ContainerKind::Rtp => Ok("application/rtp"),
    }
}

fn wait_for_frame(source: &mut dyn EncodedFrameSource) -> anyhow::Result<EncodedFrame> {
    loop {
        match source.wait_for_read()? {
            EncodedRead::Frame(frame) => return Ok(frame),
            EncodedRead::Gap { missed } => {
                warn!("[audio] live gap while waiting for first frame: {}", missed);
            }
            EncodedRead::Empty => {}
        }
    }
}

fn require_codec_id(codec_id: Option<&str>) -> anyhow::Result<&str> {
    codec_id.ok_or_else(|| anyhow::anyhow!("missing codec_id for audio http output"))
}

fn validate_http_codec(codec_id: &str, info: &CodecInfo) -> anyhow::Result<()> {
    match info.container {
        ContainerKind::Raw | ContainerKind::Ogg | ContainerKind::Mpeg => Ok(()),
        ContainerKind::Rtp => Err(anyhow::anyhow!(
            "audio http output does not accept RTP container (codec_id '{}', container {:?})",
            codec_id,
            info.container
        )),
    }
}
