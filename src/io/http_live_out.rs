// src/io/http_live_out.rs
use std::io::Read;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::thread;
use std::time::Duration;

use log::{error, info, warn};
use tiny_http::{Header, Request, Response, StatusCode};

use crate::codecs::{AudioCodec, CodecConfig};
use crate::io::http_service::HttpAudioOutput;
use crate::ring::{RingRead, RingReader};

pub struct HttpLiveOutput {
    route: String,
    codec: CodecConfig,
    ring_factory: Arc<dyn Fn() -> RingReader + Send + Sync>,
}

impl HttpLiveOutput {
    pub fn new(
        route: String,
        codec: CodecConfig,
        ring_factory: Arc<dyn Fn() -> RingReader + Send + Sync>,
    ) -> Self {
        Self {
            route,
            codec,
            ring_factory,
        }
    }
}

impl HttpAudioOutput for HttpLiveOutput {
    fn matches(&self, url: &str) -> bool {
        path_only(url) == self.route
    }

    fn handle(&self, req: Request) {
        info!("[audio] live start {}", self.route);

        let mut codec = match self.codec.build() {
            Ok(codec) => codec,
            Err(e) => {
                error!("[audio] live codec init failed: {}", e);
                let _ = req.respond(Response::empty(StatusCode(500)));
                return;
            }
        };

        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel::<Vec<u8>>();

        let reader = ChannelReader {
            rx,
            buffer: Vec::new(),
            stop: stop.clone(),
        };

        let response = Response::new(
            StatusCode(200),
            vec![
                Header::from_bytes("Content-Type", codec.content_type()).unwrap(),
                Header::from_bytes("Cache-Control", "no-store, no-cache").unwrap(),
                Header::from_bytes("Access-Control-Allow-Origin", "*").unwrap(),
            ],
            reader,
            None,
            None,
        );

        let codec_label = self.codec.label();
        thread::spawn({
            let stop = stop.clone();
            let ring_factory = self.ring_factory.clone();
            move || {
                let mut ring = ring_factory();
                info!("[audio] live feeder started ({})", codec_label);

                while !stop.load(Ordering::Relaxed) {
                    match ring.poll() {
                        RingRead::Chunk(slot) => {
                            match codec.encode_100ms(&slot.pcm) {
                                Ok(bytes) => {
                                    if !bytes.is_empty() && tx.send(bytes).is_err() {
                                        break;
                                    }
                                }
                                Err(e) => {
                                    warn!("[audio] live encode error: {}", e);
                                    break;
                                }
                            }
                        }
                        RingRead::Empty => thread::sleep(Duration::from_millis(5)),
                        RingRead::Gap { missed } => {
                            warn!("[audio] live gap missed={}", missed);
                        }
                    }
                }

                stop.store(true, Ordering::Relaxed);
                info!("[audio] live feeder exiting");
            }
        });

        if req.respond(response).is_err() {
            stop.store(true, Ordering::Relaxed);
        }
    }
}

struct ChannelReader {
    rx: mpsc::Receiver<Vec<u8>>,
    buffer: Vec<u8>,
    stop: Arc<AtomicBool>,
}

impl Read for ChannelReader {
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
            Err(mpsc::RecvTimeoutError::Timeout) => Err(std::io::ErrorKind::WouldBlock.into()),
            Err(mpsc::RecvTimeoutError::Disconnected) => Ok(0),
        }
    }
}

fn path_only(url: &str) -> &str {
    url.split('?').next().unwrap_or(url)
}
