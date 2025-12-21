use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{anyhow, Result};
use opus::Application;
use serde::Serialize;

#[cfg(feature = "mp3")]
use crate::codecs::mp3::Mp3Encoder;
use crate::codecs::opus::{OggOpusEncoder, OpusWebRtcEncoder};
use crate::codecs::pcm::PcmCodec;
use crate::codecs::vorbis::VorbisEncoder;
use crate::codecs::{AudioCodec, PCM_CHANNELS, PCM_FRAME_MS, PCM_SAMPLE_RATE};
use crate::config::{CodecInstanceConfig, CodecType, Config};
use crate::control::{ModuleSnapshot, ModuleState};

pub const DEFAULT_CODEC_PCM_ID: &str = "codec_pcm";
pub const DEFAULT_CODEC_OPUS_OGG_ID: &str = "codec_opus_ogg";
pub const DEFAULT_CODEC_OPUS_WEBRTC_ID: &str = "codec_opus_webrtc";
pub const DEFAULT_CODEC_MP3_ID: &str = "codec_mp3";
pub const DEFAULT_CODEC_VORBIS_ID: &str = "codec_vorbis";

#[derive(Clone, Serialize)]
pub struct CodecMetricsSnapshot {
    pub frames: u64,
    pub bytes: u64,
}

#[derive(Default)]
struct CodecMetrics {
    frames: AtomicU64,
    bytes: AtomicU64,
}

impl CodecMetrics {
    fn snapshot(&self) -> CodecMetricsSnapshot {
        CodecMetricsSnapshot {
            frames: self.frames.load(Ordering::Relaxed),
            bytes: self.bytes.load(Ordering::Relaxed),
        }
    }

    fn mark_encoded(&self, frames: u64, bytes: u64) {
        if frames > 0 {
            self.frames.fetch_add(frames, Ordering::Relaxed);
        }
        if bytes > 0 {
            self.bytes.fetch_add(bytes, Ordering::Relaxed);
        }
    }
}

pub struct CodecInstance {
    pub id: String,
    pub config: CodecInstanceConfig,
    pub module: ModuleState,
    metrics: CodecMetrics,
    last_error: Mutex<Option<String>>,
}

impl CodecInstance {
    fn new(config: CodecInstanceConfig) -> Self {
        let module = ModuleState::default();
        module.set_enabled(true);
        Self {
            id: config.id.clone(),
            config,
            module,
            metrics: CodecMetrics::default(),
            last_error: Mutex::new(None),
        }
    }

    pub fn mark_ready(&self) {
        self.module.set_running(true);
        self.module.set_connected(true);
    }

    pub fn mark_idle(&self) {
        self.module.set_connected(false);
        self.module.set_running(false);
    }

    pub fn mark_encoded(&self, input_frames: u64, output_frames: u64, bytes: u64) {
        self.module.mark_rx(input_frames);
        self.module.mark_tx(output_frames);
        self.metrics.mark_encoded(output_frames, bytes);
    }

    pub fn mark_error(&self, err: &str) {
        self.module.mark_error(1);
        let mut last_error = self.last_error.lock().expect("codec last_error lock");
        *last_error = Some(err.to_string());
    }

    pub fn snapshot(&self) -> CodecInstanceSnapshot {
        CodecInstanceSnapshot {
            id: self.id.clone(),
            codec_type: self.config.codec_type.clone(),
            config: self.config.clone(),
            runtime_state: self.module.snapshot(),
            metrics: self.metrics.snapshot(),
            last_error: self.last_error.lock().expect("codec last_error lock").clone(),
        }
    }
}

#[derive(Clone, Serialize)]
pub struct CodecInstanceSnapshot {
    pub id: String,
    pub codec_type: CodecType,
    pub config: CodecInstanceConfig,
    pub runtime_state: ModuleSnapshot,
    pub metrics: CodecMetricsSnapshot,
    pub last_error: Option<String>,
}

pub struct CodecRegistry {
    instances: HashMap<String, Arc<CodecInstance>>,
}

impl CodecRegistry {
    pub fn from_config(cfg: &Config) -> Self {
        Self::new(resolve_codec_configs(cfg))
    }

    pub fn new(configs: Vec<CodecInstanceConfig>) -> Self {
        let instances = configs
            .into_iter()
            .map(|config| {
                let instance = Arc::new(CodecInstance::new(config));
                (instance.id.clone(), instance)
            })
            .collect();
        Self { instances }
    }

    pub fn list(&self) -> Vec<Arc<CodecInstance>> {
        self.instances.values().cloned().collect()
    }

    pub fn snapshots(&self) -> Vec<CodecInstanceSnapshot> {
        self.instances
            .values()
            .map(|instance| instance.snapshot())
            .collect()
    }

    pub fn build_codec(&self, id: &str) -> Result<(Box<dyn AudioCodec>, Arc<CodecInstance>)> {
        let instance = self
            .instances
            .get(id)
            .cloned()
            .ok_or_else(|| anyhow!("codec instance '{}' not found", id))?;

        let codec = build_codec_from_config(&instance.config)?;
        Ok((codec, instance))
    }
}

pub fn resolve_codec_configs(cfg: &Config) -> Vec<CodecInstanceConfig> {
    if !cfg.codecs.is_empty() {
        return cfg.codecs.clone();
    }

    let opus_bitrate = cfg
        .icecast_out
        .as_ref()
        .map(|c| c.bitrate as u32)
        .unwrap_or(96_000);
    #[cfg(feature = "mp3")]
    let mp3_bitrate = cfg
        .mp3_out
        .as_ref()
        .map(|c| c.bitrate)
        .or_else(|| cfg.recorder.as_ref().and_then(|r| r.mp3.as_ref().map(|m| m.bitrate)))
        .unwrap_or(128);

    vec![
        CodecInstanceConfig {
            id: DEFAULT_CODEC_PCM_ID.to_string(),
            codec_type: CodecType::Pcm,
            sample_rate: Some(PCM_SAMPLE_RATE),
            channels: Some(PCM_CHANNELS),
            frame_size_ms: Some(PCM_FRAME_MS),
            bitrate: None,
            container: Some("raw".to_string()),
            mode: None,
            application: None,
            quality: None,
        },
        CodecInstanceConfig {
            id: DEFAULT_CODEC_OPUS_OGG_ID.to_string(),
            codec_type: CodecType::OpusOgg,
            sample_rate: Some(PCM_SAMPLE_RATE),
            channels: Some(PCM_CHANNELS),
            frame_size_ms: Some(20),
            bitrate: Some(opus_bitrate),
            container: Some("ogg".to_string()),
            mode: Some("stream".to_string()),
            application: Some("audio".to_string()),
            quality: None,
        },
        CodecInstanceConfig {
            id: DEFAULT_CODEC_OPUS_WEBRTC_ID.to_string(),
            codec_type: CodecType::OpusWebrtc,
            sample_rate: Some(PCM_SAMPLE_RATE),
            channels: Some(PCM_CHANNELS),
            frame_size_ms: Some(20),
            bitrate: Some(opus_bitrate),
            container: Some("rtp".to_string()),
            mode: Some("webrtc".to_string()),
            application: Some("voip".to_string()),
            quality: None,
        },
        #[cfg(feature = "mp3")]
        CodecInstanceConfig {
            id: DEFAULT_CODEC_MP3_ID.to_string(),
            codec_type: CodecType::Mp3,
            sample_rate: Some(PCM_SAMPLE_RATE),
            channels: Some(PCM_CHANNELS),
            frame_size_ms: Some(PCM_FRAME_MS),
            bitrate: Some(mp3_bitrate),
            container: Some("mpeg".to_string()),
            mode: None,
            application: None,
            quality: None,
        },
        CodecInstanceConfig {
            id: DEFAULT_CODEC_VORBIS_ID.to_string(),
            codec_type: CodecType::Vorbis,
            sample_rate: Some(PCM_SAMPLE_RATE),
            channels: Some(PCM_CHANNELS),
            frame_size_ms: Some(PCM_FRAME_MS),
            bitrate: None,
            container: Some("ogg".to_string()),
            mode: None,
            application: None,
            quality: Some(0.4),
        },
    ]
}

fn build_codec_from_config(config: &CodecInstanceConfig) -> Result<Box<dyn AudioCodec>> {
    let sample_rate = config.sample_rate.unwrap_or(PCM_SAMPLE_RATE);
    let bitrate = config.bitrate.unwrap_or(96_000);
    let application = config.application.as_deref().unwrap_or("audio");
    let quality = config.quality.unwrap_or(0.4);
    let opus_application = if application.eq_ignore_ascii_case("voip") {
        Application::Voip
    } else {
        Application::Audio
    };

    match config.codec_type {
        CodecType::Pcm => Ok(Box::new(PcmCodec::new())),
        CodecType::OpusOgg => {
            Ok(Box::new(OggOpusEncoder::new_with_application(
                bitrate as i32,
                "airlift",
                opus_application,
            )?))
        }
        CodecType::OpusWebrtc => {
            Ok(Box::new(OpusWebRtcEncoder::new_with_application(
                bitrate as i32,
                opus_application,
            )?))
        }
        CodecType::Mp3 => {
            #[cfg(feature = "mp3")]
            {
                Ok(Box::new(Mp3Encoder::new(bitrate, sample_rate)?))
            }
            #[cfg(not(feature = "mp3"))]
            {
                Err(anyhow!("mp3 codec not enabled"))
            }
        }
        CodecType::Vorbis => Ok(Box::new(VorbisEncoder::new(quality)?)),
        CodecType::AacLc | CodecType::Flac => {
            Err(anyhow!("codec '{}' not implemented", config.id))
        }
    }
}
