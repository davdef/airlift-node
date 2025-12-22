use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use opus::Application;
use serde::Serialize;

#[cfg(feature = "mp3")]
use crate::codecs::mp3::Mp3Encoder;
use crate::codecs::opus::{OggOpusEncoder, OpusWebRtcEncoder};
use crate::codecs::pcm::PcmCodec;
use crate::codecs::vorbis::VorbisEncoder;
use crate::codecs::{
    AudioCodec, CodecInfo, CodecKind, ContainerKind, PCM_CHANNELS, PCM_SAMPLE_RATE,
};
use crate::config::{CodecInstanceConfig, CodecType, Config};
use crate::control::{ModuleSnapshot, ModuleState};

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
            info: codec_info_from_config(&self.config),
            runtime_state: self.module.snapshot(),
            metrics: self.metrics.snapshot(),
            last_error: self
                .last_error
                .lock()
                .expect("codec last_error lock")
                .clone(),
        }
    }
}

#[derive(Clone, Serialize)]
pub struct CodecInstanceSnapshot {
    pub id: String,
    pub codec_type: CodecType,
    pub config: CodecInstanceConfig,
    pub info: CodecInfo,
    pub runtime_state: ModuleSnapshot,
    pub metrics: CodecMetricsSnapshot,
    pub last_error: Option<String>,
}

pub struct CodecRegistry {
    instances: HashMap<String, Arc<CodecInstance>>,
    codecs: Mutex<HashMap<String, Arc<Mutex<Box<dyn AudioCodec>>>>>,
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
        Self {
            instances,
            codecs: Mutex::new(HashMap::new()),
        }
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

    pub fn build_codec_handle(
        &self,
        id: &str,
    ) -> Result<(Arc<Mutex<Box<dyn AudioCodec>>>, Arc<CodecInstance>)> {
        let instance = self
            .instances
            .get(id)
            .cloned()
            .ok_or_else(|| anyhow!("codec instance '{}' not found", id))?;

        let mut codecs = self.codecs.lock().expect("codec registry lock");
        if let Some(codec) = codecs.get(id) {
            return Ok((codec.clone(), instance));
        }

        let codec = Arc::new(Mutex::new(build_codec_from_config(&instance.config)?));
        codecs.insert(id.to_string(), codec.clone());
        Ok((codec, instance))
    }

    pub fn get_instance(&self, id: &str) -> Result<Arc<CodecInstance>> {
        self.instances
            .get(id)
            .cloned()
            .ok_or_else(|| anyhow!("codec instance '{}' not found", id))
    }

    pub fn get_info(&self, id: &str) -> Result<CodecInfo> {
        let instance = self
            .instances
            .get(id)
            .ok_or_else(|| anyhow!("codec instance '{}' not found", id))?;
        Ok(codec_info_from_config(&instance.config))
    }
}

pub fn resolve_codec_configs(cfg: &Config) -> Vec<CodecInstanceConfig> {
    cfg.codec_instances()
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
        CodecType::OpusOgg => Ok(Box::new(OggOpusEncoder::new_with_application(
            bitrate as i32,
            "airlift",
            opus_application,
        )?)),
        CodecType::OpusWebrtc => Ok(Box::new(OpusWebRtcEncoder::new_with_application(
            bitrate as i32,
            opus_application,
        )?)),
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
        CodecType::AacLc | CodecType::Flac => Err(anyhow!("codec '{}' not implemented", config.id)),
    }
}

fn codec_info_from_config(config: &CodecInstanceConfig) -> CodecInfo {
    CodecInfo {
        kind: codec_kind_from_type(&config.codec_type),
        sample_rate: config.sample_rate.unwrap_or(PCM_SAMPLE_RATE),
        channels: config.channels.unwrap_or(PCM_CHANNELS),
        container: container_from_config(config),
    }
}

fn codec_kind_from_type(codec_type: &CodecType) -> CodecKind {
    match codec_type {
        CodecType::Pcm => CodecKind::Pcm,
        CodecType::OpusOgg => CodecKind::OpusOgg,
        CodecType::OpusWebrtc => CodecKind::OpusWebRtc,
        CodecType::Mp3 => CodecKind::Mp3,
        CodecType::Vorbis => CodecKind::Vorbis,
        CodecType::AacLc => CodecKind::AacLc,
        CodecType::Flac => CodecKind::Flac,
    }
}

fn container_from_config(config: &CodecInstanceConfig) -> ContainerKind {
    if let Some(container) = config.container.as_deref() {
        return match container.to_ascii_lowercase().as_str() {
            "raw" => ContainerKind::Raw,
            "ogg" => ContainerKind::Ogg,
            "mpeg" => ContainerKind::Mpeg,
            "rtp" => ContainerKind::Rtp,
            _ => default_container_for_type(&config.codec_type),
        };
    }

    default_container_for_type(&config.codec_type)
}

fn default_container_for_type(codec_type: &CodecType) -> ContainerKind {
    match codec_type {
        CodecType::Pcm | CodecType::AacLc | CodecType::Flac => ContainerKind::Raw,
        CodecType::OpusOgg | CodecType::Vorbis => ContainerKind::Ogg,
        CodecType::OpusWebrtc => ContainerKind::Rtp,
        CodecType::Mp3 => ContainerKind::Mpeg,
    }
}
