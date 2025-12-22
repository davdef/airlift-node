use std::io::{BufReader, Read, Seek, SeekFrom};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use log::{info, warn};
use symphonia::core::audio::{AudioBufferRef, SampleBuffer};
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::{MediaSource, MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::default::{get_codecs, get_probe};

use crate::codecs::{PCM_I16_SAMPLES, PCM_SAMPLE_RATE, PCM_SAMPLES_PER_CH};
use crate::control::ModuleState;
use crate::ring::AudioRing;

const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
const READ_BUFFER_SIZE: usize = 64 * 1024;

pub fn run_icecast_in(
    ring: AudioRing,
    url: String,
    running: Arc<AtomicBool>,
    state: Arc<ModuleState>,
    ring_state: Arc<ModuleState>,
) -> Result<()> {
    state.set_running(true);
    state.set_connected(false);

    let mut backoff = INITIAL_BACKOFF;
    while running.load(Ordering::Relaxed) {
        match connect_and_stream(
            &ring,
            &url,
            running.clone(),
            state.clone(),
            ring_state.clone(),
        ) {
            Ok(()) => {
                state.set_connected(false);
                backoff = INITIAL_BACKOFF;
            }
            Err(e) => {
                warn!("[icecast_in] error: {}", e);
                state.mark_error(1);
                state.set_connected(false);
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
        }
    }

    state.set_running(false);
    state.set_connected(false);
    Ok(())
}

fn connect_and_stream(
    ring: &AudioRing,
    url: &str,
    running: Arc<AtomicBool>,
    state: Arc<ModuleState>,
    ring_state: Arc<ModuleState>,
) -> Result<()> {
    info!("[icecast_in] connecting to {}", url);

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(Duration::from_secs(10))
        .build();

    let response = agent
        .get(url)
        .call()
        .map_err(|e| anyhow!("http error: {}", e))?;

    if response.status() >= 400 {
        return Err(anyhow!(
            "http status {} {}",
            response.status(),
            response.status_text()
        ));
    }

    state.set_connected(true);

    let reader = BufReader::with_capacity(READ_BUFFER_SIZE, response.into_reader());
    let source = NonSeekableSource::new(reader);
    let mss = MediaSourceStream::new(
        Box::new(source),
        MediaSourceStreamOptions {
            seekable: false,
            ..Default::default()
        },
    );

    let mut hint = Hint::new();
    if let Some(ext) = url.split('.').last().filter(|ext| ext.len() <= 8) {
        hint.with_extension(ext);
    }

    let mut probed = get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .context("failed to probe icecast stream")?;
    let track = probed
        .format
        .default_track()
        .ok_or_else(|| anyhow!("no audio tracks found"))?;
    let mut decoder = get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .context("failed to create decoder")?;

    let mut chunker = PcmChunker::default();
    let mut ts_state = PcmTimestampState::default();

    while running.load(Ordering::Relaxed) {
        let packet = match probed.format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(err))
                if err.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                return Err(anyhow!("stream ended"));
            }
            Err(SymphoniaError::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(err) => return Err(err).context("failed reading packet"),
        };

        match decoder.decode(&packet) {
            Ok(audio_buf) => {
                let pcm = decode_to_pcm_i16(audio_buf)?;
                chunker.push_frames(&pcm, ring, &mut ts_state, &state, &ring_state);
            }
            Err(SymphoniaError::DecodeError(err)) => {
                warn!("[icecast_in] decode error: {}", err);
            }
            Err(err) => return Err(err).context("decoder error"),
        }
    }

    Ok(())
}

fn decode_to_pcm_i16(buffer: AudioBufferRef<'_>) -> Result<Vec<i16>> {
    let spec = *buffer.spec();
    let mut sample_buf = SampleBuffer::<f32>::new(buffer.capacity() as u64, spec);
    sample_buf.copy_interleaved_ref(buffer);
    let channels = sample_buf.spec().channels.count();
    let in_rate = sample_buf.spec().rate;
    let frames = sample_buf.frames();
    let samples = sample_buf.samples();

    if channels == 0 || frames == 0 {
        return Ok(Vec::new());
    }

    let out_frames = if in_rate == PCM_SAMPLE_RATE {
        frames
    } else {
        let ratio = PCM_SAMPLE_RATE as f64 / in_rate as f64;
        (frames as f64 * ratio).round() as usize
    };

    let mut out = Vec::with_capacity(out_frames * 2);
    for out_idx in 0..out_frames {
        let src_pos = if in_rate == PCM_SAMPLE_RATE {
            out_idx as f64
        } else {
            out_idx as f64 * in_rate as f64 / PCM_SAMPLE_RATE as f64
        };
        let base = src_pos.floor() as usize;
        let frac = (src_pos - base as f64) as f32;
        let idx0 = base.min(frames.saturating_sub(1));
        let idx1 = (base + 1).min(frames.saturating_sub(1));
        let (l0, r0) = stereo_from_frame(samples, channels, idx0);
        let (l1, r1) = stereo_from_frame(samples, channels, idx1);
        let left = l0 + (l1 - l0) * frac;
        let right = r0 + (r1 - r0) * frac;
        out.push(float_to_i16(left));
        out.push(float_to_i16(right));
    }

    Ok(out)
}

fn stereo_from_frame(samples: &[f32], channels: usize, frame: usize) -> (f32, f32) {
    match channels {
        0 => (0.0, 0.0),
        1 => {
            let value = samples[frame];
            (value, value)
        }
        _ => {
            let base = frame * channels;
            (samples[base], samples[base + 1])
        }
    }
}

fn float_to_i16(value: f32) -> i16 {
    let clamped = value.clamp(-1.0, 1.0);
    (clamped * i16::MAX as f32) as i16
}

#[derive(Default)]
struct PcmChunker {
    pending: Vec<i16>,
}

impl PcmChunker {
    fn push_frames(
        &mut self,
        pcm: &[i16],
        ring: &AudioRing,
        ts_state: &mut PcmTimestampState,
        state: &ModuleState,
        ring_state: &ModuleState,
    ) {
        if pcm.is_empty() {
            return;
        }
        self.pending.extend_from_slice(pcm);
        while self.pending.len() >= PCM_I16_SAMPLES {
            let frame: Vec<i16> = self.pending.drain(..PCM_I16_SAMPLES).collect();
            let utc_ns = ts_state.next_frame_utc_ns();
            ring.writer_push(utc_ns, frame);
            state.mark_rx(1);
            ring_state.mark_rx(1);
        }
    }
}

#[derive(Default)]
struct PcmTimestampState {
    start_utc_ns: Option<u64>,
    samples_emitted: u64,
}

impl PcmTimestampState {
    fn next_frame_utc_ns(&mut self) -> u64 {
        let start = self.start_utc_ns.get_or_insert_with(now_utc_ns);
        let offset_ns = self.samples_emitted * 1_000_000_000 / PCM_SAMPLE_RATE as u64;
        // Icecast liefert keine RFMA-Zeitstempel wie SRT-In; wir synthetisieren lokal
        // aus Startzeit + Sample-Fortschritt (Fallback).
        let utc_ns = *start + offset_ns;
        self.samples_emitted += PCM_SAMPLES_PER_CH as u64;
        utc_ns
    }
}

struct NonSeekableSource<R> {
    reader: R,
}

impl<R> NonSeekableSource<R> {
    fn new(reader: R) -> Self {
        Self { reader }
    }
}

impl<R: Read + Send + Sync> Read for NonSeekableSource<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.reader.read(buf)
    }
}

impl<R: Read + Send + Sync> Seek for NonSeekableSource<R> {
    fn seek(&mut self, _pos: SeekFrom) -> std::io::Result<u64> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "icecast stream is not seekable",
        ))
    }
}

impl<R: Read + Send + Sync> MediaSource for NonSeekableSource<R> {
    fn is_seekable(&self) -> bool {
        false
    }

    fn byte_len(&self) -> Option<u64> {
        None
    }
}

fn now_utc_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}
