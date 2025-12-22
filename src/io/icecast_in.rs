// src/io/icecast_in.rs

use std::io::{BufReader, Read, Seek, SeekFrom};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use log::{info, warn};
use opus::{Decoder as OpusDecoder, Channels as OpusChannels};

use symphonia::core::audio::{AudioBufferRef, SampleBuffer};
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_OPUS};
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
const MAX_OPUS_FRAME_SAMPLES: usize = 5760;

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
            buffer_len: READ_BUFFER_SIZE,
        },
    );

    let mut hint = Hint::new();
    if let Some(ext) = url.split('.').last().filter(|e| e.len() <= 8) {
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

    let mut chunker = PcmChunker::default();
    let mut ts_state = PcmTimestampState::default();

    if track.codec_params.codec == Some(CODEC_TYPE_OPUS) {
        if let Err(err) = stream_opus_packets(
            &mut probed,
            track.id,
            ring,
            running,
            state.clone(),
            ring_state.clone(),
            &mut chunker,
            &mut ts_state,
        ) {
            warn!("[icecast_in] opus decode error: {}", err);
            return Err(err).context("opus decode error");
        }

        return Ok(());
    }

    let mut decoder = get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .context("failed to create decoder")?;

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

fn stream_opus_packets(
    probed: &mut symphonia::core::probe::Probed,
    track_id: u32,
    ring: &AudioRing,
    running: Arc<AtomicBool>,
    state: Arc<ModuleState>,
    ring_state: Arc<ModuleState>,
    chunker: &mut PcmChunker,
    ts_state: &mut PcmTimestampState,
) -> Result<()> {
    let mut decoder: Option<OpusDecoder> = None;
    let mut channels: u8 = 0;
    let mut pre_skip_remaining: usize = 0;
    let mut saw_tags = false;
    let mut decode_buf: Vec<i16> = Vec::new();

    while running.load(Ordering::Relaxed) {
        let packet = match probed.format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(err))
                if err.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                return Err(anyhow!("stream ended"));
            }
            Err(SymphoniaError::ResetRequired) => {
                decoder = None;
                channels = 0;
                pre_skip_remaining = 0;
                saw_tags = false;
                continue;
            }
            Err(err) => return Err(err).context("failed reading packet"),
        };

        if packet.track_id() != track_id {
            continue;
        }

        if decoder.is_none() {
            if let Some(header) = parse_opus_head(packet.buf()) {
                channels = header.channels;
                pre_skip_remaining = header.pre_skip as usize;
                decoder = Some(
                    OpusDecoder::new(PCM_SAMPLE_RATE, opus_channels(header.channels)?)
                        .context("failed to create opus decoder")?,
                );
                decode_buf = vec![0i16; MAX_OPUS_FRAME_SAMPLES * channels as usize];
                continue;
            }
            continue;
        }

        if !saw_tags && is_opus_tags(packet.buf()) {
            saw_tags = true;
            continue;
        }

        let decoder = decoder.as_mut().expect("decoder initialized");
        let frame_samples = decoder
            .decode(packet.buf(), &mut decode_buf, false)
            .context("opus decode failed")?;
        let decoded_samples = frame_samples * channels as usize;
        if decoded_samples == 0 {
            continue;
        }

        let mut pcm = &decode_buf[..decoded_samples];
        if pre_skip_remaining > 0 {
            let skip = pre_skip_remaining.min(frame_samples);
            let skip_samples = skip * channels as usize;
            if skip_samples >= pcm.len() {
                pre_skip_remaining -= skip;
                continue;
            }
            pcm = &pcm[skip_samples..];
            pre_skip_remaining -= skip;
        }

        let stereo_pcm = opus_to_stereo(pcm, channels);
        chunker.push_frames(&stereo_pcm, ring, ts_state, &state, &ring_state);
    }

    Ok(())
}

fn parse_opus_head(data: &[u8]) -> Option<OpusHead> {
    if data.len() < 19 || &data[..8] != b"OpusHead" {
        return None;
    }

    let channels = data[9];
    let pre_skip = u16::from_le_bytes([data[10], data[11]]);
    Some(OpusHead { channels, pre_skip })
}

fn is_opus_tags(data: &[u8]) -> bool {
    data.len() >= 8 && &data[..8] == b"OpusTags"
}

fn opus_channels(channels: u8) -> Result<OpusChannels> {
    match channels {
        1 => Ok(OpusChannels::Mono),
        2 => Ok(OpusChannels::Stereo),
        _ => Err(anyhow!("unsupported opus channel count {}", channels)),
    }
}

fn opus_to_stereo(samples: &[i16], channels: u8) -> Vec<i16> {
    match channels {
        1 => samples
            .iter()
            .flat_map(|sample| [*sample, *sample])
            .collect(),
        _ => {
            let mut out = Vec::with_capacity(samples.len());
            let mut idx = 0;
            while idx + 1 < samples.len() {
                out.push(samples[idx]);
                out.push(samples[idx + 1]);
                idx += channels as usize;
            }
            out
        }
    }
}

struct OpusHead {
    channels: u8,
    pre_skip: u16,
}

fn decode_to_pcm_i16(buffer: AudioBufferRef<'_>) -> Result<Vec<i16>> {
    let spec = *buffer.spec();
    let channels = spec.channels.count();
    let in_rate = spec.rate;
    let frames = buffer.frames();

    if channels == 0 || frames == 0 {
        return Ok(Vec::new());
    }

    let mut sample_buf = SampleBuffer::<f32>::new(frames as u64, spec);
    sample_buf.copy_interleaved_ref(buffer);
    let samples = sample_buf.samples();

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
            let v = samples[frame];
            (v, v)
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
        let offset_ns =
            self.samples_emitted * 1_000_000_000 / PCM_SAMPLE_RATE as u64;
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
