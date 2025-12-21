// src/io/http_timeshift_out.rs
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
use crate::io::timeshift::stream_timeshift;

pub struct HttpTimeshiftOutput {
    route: String,
    codec: CodecConfig,
    wav_dir: Arc<std::path::PathBuf>,
}

impl HttpTimeshiftOutput {
    pub fn new(route: String, codec: CodecConfig, wav_dir: Arc<std::path::PathBuf>) -> Self {
        Self {
            route,
            codec,
            wav_dir,
        }
    }
}

impl HttpAudioOutput for HttpTimeshiftOutput {
    fn matches(&self, url: &str) -> bool {
        path_only(url) == self.route
    }

    fn handle(&self, req: Request) {
        let ts = match extract_ts(req.url()) {
            Some(ts) => ts,
            None => {
                let _ = req.respond(Response::from_string("missing ts").with_status_code(400));
                return;
            }
        };

        let codec_label = self.codec.label();
        info!("[audio] timeshift start ts={} ({})", ts, self.route);

        let mut codec = match self.codec.build() {
            Ok(codec) => codec,
            Err(e) => {
                error!("[audio] timeshift codec init failed: {}", e);
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

        thread::spawn({
            let stop = stop.clone();
            let wav_dir = self.wav_dir.clone();
            move || {
                let result = stream_timeshift(wav_dir.as_ref().clone(), ts, |pcm| {
                    if stop.load(Ordering::Relaxed) {
                        return Ok(());
                    }
                    let bytes = codec.encode_100ms(pcm)?;
                    if !bytes.is_empty() && tx.send(bytes).is_err() {
                        stop.store(true, Ordering::Relaxed);
                    }
                    Ok(())
                });

                if let Err(e) = result {
                    warn!("[audio] timeshift stream error: {}", e);
                }

                stop.store(true, Ordering::Relaxed);
                info!("[audio] timeshift feeder exiting ({})", codec_label);
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

fn extract_ts(url: &str) -> Option<u64> {
    url.split('?')
        .nth(1)?
        .split('&')
        .find_map(|p| {
            let mut it = p.split('=');
            (it.next()? == "ts").then(|| it.next()?.parse().ok()).flatten()
        })
}
