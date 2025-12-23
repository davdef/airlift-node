use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::codecs::{CodecInfo, CodecKind, ContainerKind};

// ---------- Ring ----------
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RingConfig {
    #[serde(default = "default_ring_slots")]
    pub slots: usize,
    #[serde(default = "default_ring_prealloc_samples")]
    pub prealloc_samples: usize,
}

impl Default for RingConfig {
    fn default() -> Self {
        Self {
            slots: 6000,
            prealloc_samples: 9600,
        }
    }
}

fn default_ring_slots() -> usize {
    RingConfig::default().slots
}

fn default_ring_prealloc_samples() -> usize {
    RingConfig::default().prealloc_samples
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RingBufferConfig {
    pub slots: usize,
    pub prealloc_samples: usize,
}

// ---------- ALSA ----------
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AlsaInConfig {
    pub enabled: bool,
    pub device: String,
}

// ---------- UDP ----------
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct UdpOutConfig {
    pub enabled: bool,
    pub target: String,
    #[serde(default)]
    pub codec_id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SrtOutConfig {
    pub enabled: bool,
    pub target: String, // "host:port"
    pub latency_ms: u32,
    #[serde(default)]
    pub codec_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RecorderConfigToml {
    pub enabled: bool,
    pub wav_dir: String,
    pub retention_days: u64,
    pub mp3: Option<Mp3ConfigToml>,
    #[serde(default)]
    pub codec_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Mp3ConfigToml {
    pub dir: String,
    pub bitrate: u32,
}

// ---------- Icecast (Opus) ----------
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct IcecastOutConfig {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub mount: String,
    pub user: String,
    pub password: String,
    pub bitrate: i32,
    pub name: String,
    pub description: String,
    pub genre: String,
    pub public: bool,
    #[serde(default)]
    pub codec_id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct InputConfig {
    #[serde(rename = "type")]
    pub input_type: String,
    pub enabled: bool,
    pub buffer: String,
    pub listen: Option<String>,
    pub latency_ms: Option<u32>,
    pub streamid: Option<String>,
    pub device: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct OutputConfig {
    #[serde(rename = "type")]
    pub output_type: String,
    pub enabled: bool,
    pub input: Option<String>,
    pub buffer: String,
    pub codec_id: Option<String>,
    pub target: Option<String>,
    pub latency_ms: Option<u32>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub mount: Option<String>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub genre: Option<String>,
    pub public: Option<bool>,
    pub bitrate: Option<u32>,
    pub wav_dir: Option<String>,
    pub retention_days: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ServiceConfig {
    #[serde(rename = "type")]
    pub service_type: String,
    pub enabled: bool,
    pub input: Option<String>,
    pub buffer: Option<String>,
    pub codec_id: Option<String>,
    pub url: Option<String>,
    pub db: Option<String>,
    pub interval_ms: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum CodecType {
    Pcm,
    OpusOgg,
    OpusWebrtc,
    Mp3,
    Vorbis,
    AacLc,
    Flac,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CodecInstanceConfig {
    pub id: String,
    #[serde(rename = "type")]
    pub codec_type: CodecType,
    #[serde(default)]
    pub sample_rate: Option<u32>,
    #[serde(default)]
    pub channels: Option<u8>,
    #[serde(default)]
    pub frame_size_ms: Option<u32>,
    #[serde(default)]
    pub bitrate: Option<u32>,
    #[serde(default)]
    pub container: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub application: Option<String>,
    #[serde(default)]
    pub quality: Option<f32>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CodecInstanceConfigEntry {
    #[serde(rename = "type")]
    pub codec_type: CodecType,
    #[serde(default)]
    pub sample_rate: Option<u32>,
    #[serde(default)]
    pub channels: Option<u8>,
    #[serde(default)]
    pub frame_size_ms: Option<u32>,
    #[serde(default)]
    pub bitrate: Option<u32>,
    #[serde(default)]
    pub container: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub application: Option<String>,
    #[serde(default)]
    pub quality: Option<f32>,
}

impl CodecInstanceConfigEntry {
    fn into_instance(self, id: String) -> CodecInstanceConfig {
        CodecInstanceConfig {
            id,
            codec_type: self.codec_type,
            sample_rate: self.sample_rate,
            channels: self.channels,
            frame_size_ms: self.frame_size_ms,
            bitrate: self.bitrate,
            container: self.container,
            mode: self.mode,
            application: self.application,
            quality: self.quality,
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(untagged)]
pub enum CodecConfigs {
    List(Vec<CodecInstanceConfig>),
    Map(BTreeMap<String, CodecInstanceConfigEntry>),
}

impl Default for CodecConfigs {
    fn default() -> Self {
        CodecConfigs::List(Vec::new())
    }
}

// ---------- Recorder ----------
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct HourlyRecorderConfig {
    pub enabled: bool,
    pub base_dir: String,
}

// ---------- Monitoring ----------
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MonitoringConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_monitoring_port")]
    pub http_port: u16,
}

impl Default for MonitoringConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            http_port: 9090,
        }
    }
}

fn default_monitoring_port() -> u16 {
    MonitoringConfig::default().http_port
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ApiConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_api_bind")]
    pub bind: String,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bind: "0.0.0.0:3008".to_string(),
        }
    }
}

fn default_api_bind() -> String {
    ApiConfig::default().bind
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AudioHttpConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_audio_http_bind")]
    pub bind: String,
    #[serde(default)]
    pub codec_id: Option<String>,
}

impl Default for AudioHttpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bind: "0.0.0.0:3011".to_string(),
            codec_id: None,
        }
    }
}

fn default_audio_http_bind() -> String {
    AudioHttpConfig::default().bind
}

// ---------- Metadata ----------
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct MetadataConfig {
    pub default: String,
}

// ---------- Influx History ----------
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct InfluxHistoryConfig {
    pub enabled: bool,
    pub base_url: String,
    pub token: String,
    pub org: String,
    pub bucket: String,
}

// ---------- Root ----------
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Config {
    #[serde(default)]
    pub ring: RingConfig,
    pub alsa_in: Option<AlsaInConfig>,
    pub udp_out: Option<UdpOutConfig>,
    pub icecast_out: Option<IcecastOutConfig>,
    pub srt_in: Option<SrtInConfig>,
    pub srt_out: Option<SrtOutConfig>,
    pub recorder: Option<RecorderConfigToml>,
    #[serde(default)]
    pub monitoring: MonitoringConfig,
    #[serde(default)]
    pub api: ApiConfig,
    #[serde(default)]
    pub audio_http: AudioHttpConfig,
    #[serde(default)]
    pub metadata: MetadataConfig,
    pub influx_history: Option<InfluxHistoryConfig>,
    #[serde(default)]
    pub audio_http_codec_id: Option<String>,
    #[serde(default)]
    pub peak_storage_enabled: bool,
    #[serde(default)]
    pub codecs: CodecConfigs,
    #[serde(default)]
    pub ringbuffers: BTreeMap<String, RingBufferConfig>,
    #[serde(default)]
    pub inputs: BTreeMap<String, InputConfig>,
    #[serde(default)]
    pub outputs: BTreeMap<String, OutputConfig>,
    #[serde(default)]
    pub services: BTreeMap<String, ServiceConfig>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SrtInConfig {
    pub enabled: bool,
    pub listen: String,
    pub latency_ms: u32,
}

// ---------- Loader ----------
pub fn load(path: &str) -> anyhow::Result<Config> {
    let txt = std::fs::read_to_string(path)?;
    Ok(toml::from_str(&txt)?)
}

#[derive(Debug, Clone)]
pub struct ValidatedGraphConfig {
    pub ringbuffer_id: String,
    pub ringbuffer: RingBufferConfig,
    pub inputs: BTreeMap<String, InputConfig>,
    pub outputs: BTreeMap<String, OutputConfig>,
    pub services: BTreeMap<String, ServiceConfig>,
    pub codecs: Vec<CodecInstanceConfig>,
    pub codec_info: BTreeMap<String, CodecInfo>,
}

impl Config {
    pub fn codec_instances(&self) -> Vec<CodecInstanceConfig> {
        match &self.codecs {
            CodecConfigs::List(list) => list.clone(),
            CodecConfigs::Map(map) => map
                .iter()
                .map(|(id, entry)| entry.clone().into_instance(id.clone()))
                .collect(),
        }
    }

    pub fn has_graph_config(&self) -> bool {
        !self.ringbuffers.is_empty()
            || !self.inputs.is_empty()
            || !self.outputs.is_empty()
            || !self.services.is_empty()
    }

    pub fn uses_icecast_input(&self) -> bool {
        self.inputs
            .values()
            .any(|input| matches!(input.input_type.as_str(), "icecast" | "http_stream"))
    }

    pub fn validate_graph(&self) -> anyhow::Result<Option<ValidatedGraphConfig>> {
        if !self.has_graph_config() {
            return Ok(None);
        }

        let ringbuffer_required = ringbuffer_required(&self.outputs, &self.services);
        if ringbuffer_required {
            if self.ringbuffers.is_empty() {
                anyhow::bail!(
                    "graph config requires a ringbuffer when monitoring or parallel consumers are enabled"
                );
            }
            if self.ringbuffers.len() != 1 {
                anyhow::bail!("graph config currently supports exactly one ringbuffer");
            }
        } else if !self.ringbuffers.is_empty() {
            anyhow::bail!(
                "ringbuffers are only allowed when monitoring or parallel consumers are enabled"
            );
        }

        let (ringbuffer_id, ringbuffer) = self
            .ringbuffers
            .iter()
            .next()
            .map(|(id, cfg)| (id.clone(), cfg.clone()))
            .unwrap_or_else(|| ("".to_string(), RingBufferConfig {
                slots: self.ring.slots,
                prealloc_samples: self.ring.prealloc_samples,
            }));

        let codecs = self.codec_instances();
        let mut codec_info = BTreeMap::new();
        for codec in codecs.iter() {
            if codec_info.contains_key(&codec.id) {
                anyhow::bail!("duplicate codec_id '{}' in configuration", codec.id);
            }
            codec_info.insert(codec.id.clone(), codec_info_from_config(codec));
        }

        let ringbuffer_id = if ringbuffer_required {
            Some(ringbuffer_id.as_str())
        } else {
            None
        };
        validate_inputs(&self.inputs, ringbuffer_id)?;
        validate_outputs(&self.outputs, ringbuffer_id, &self.inputs, &codec_info)?;
        validate_services(&self.services, ringbuffer_id, &self.inputs)?;

        Ok(Some(ValidatedGraphConfig {
            ringbuffer_id: ringbuffer_id
                .unwrap_or_else(|| "")
                .to_string(),
            ringbuffer,
            inputs: self.inputs.clone(),
            outputs: self.outputs.clone(),
            services: self.services.clone(),
            codecs,
            codec_info,
        }))
    }
}

fn validate_inputs(
    inputs: &BTreeMap<String, InputConfig>,
    ringbuffer_id: Option<&str>,
) -> anyhow::Result<()> {
    let Some(ringbuffer_id) = ringbuffer_id else {
        if inputs.is_empty() {
            return Ok(());
        }
        anyhow::bail!(
            "inputs require a ringbuffer; enable monitoring or parallel consumers to use ringbuffers"
        );
    };
    let mut seen_types = BTreeMap::new();
    for (id, input) in inputs {
        if input.buffer != ringbuffer_id {
            anyhow::bail!(
                "input '{}' references unknown ringbuffer '{}'",
                id,
                input.buffer
            );
        }

        match input.input_type.as_str() {
            "srt" => {
                if input.listen.as_deref().unwrap_or("").is_empty() {
                    anyhow::bail!("input '{}' requires listen", id);
                }
                if input.latency_ms.is_none() {
                    anyhow::bail!("input '{}' requires latency_ms", id);
                }
            }
            "icecast" | "http_stream" => {
                if input.url.as_deref().unwrap_or("").is_empty() {
                    anyhow::bail!("input '{}' requires url", id);
                }
            }
            "alsa" => {
                if input.device.as_deref().unwrap_or("").is_empty() {
                    anyhow::bail!("input '{}' requires device", id);
                }
            }
            other => {
                anyhow::bail!("input '{}' has unsupported type '{}'", id, other);
            }
        }

        if seen_types.insert(input.input_type.clone(), id).is_some() {
            anyhow::bail!(
                "multiple inputs of type '{}' are not supported",
                input.input_type
            );
        }
    }
    Ok(())
}

fn validate_outputs(
    outputs: &BTreeMap<String, OutputConfig>,
    ringbuffer_id: Option<&str>,
    inputs: &BTreeMap<String, InputConfig>,
    codec_info: &BTreeMap<String, CodecInfo>,
) -> anyhow::Result<()> {
    let Some(ringbuffer_id) = ringbuffer_id else {
        if outputs.is_empty() {
            return Ok(());
        }
        anyhow::bail!(
            "outputs require a ringbuffer; enable monitoring or parallel consumers to use ringbuffers"
        );
    };
    for (id, output) in outputs {
        if output.buffer != ringbuffer_id {
            anyhow::bail!(
                "output '{}' references unknown ringbuffer '{}'",
                id,
                output.buffer
            );
        }

        let input_id = output
            .input
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("output '{}' requires input", id))?;
        let input = inputs
            .get(input_id)
            .ok_or_else(|| anyhow::anyhow!("output '{}' references unknown input '{}'", id, input_id))?;
        if input.buffer != output.buffer {
            anyhow::bail!(
                "output '{}' buffer '{}' does not match input '{}' buffer '{}'",
                id,
                output.buffer,
                input_id,
                input.buffer
            );
        }

        let codec_id = output
            .codec_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("output '{}' requires codec_id", id))?;
        let info = codec_info.get(codec_id).ok_or_else(|| {
            anyhow::anyhow!("output '{}' references unknown codec_id '{}'", id, codec_id)
        })?;

        match output.output_type.as_str() {
            "icecast_out" => {
                require_output_field(id, "host", output.host.as_ref())?;
                require_output_field(id, "port", output.port.as_ref())?;
                require_output_field(id, "mount", output.mount.as_ref())?;
                require_output_field(id, "user", output.user.as_ref())?;
                require_output_field(id, "password", output.password.as_ref())?;
                require_output_field(id, "name", output.name.as_ref())?;
                require_output_field(id, "description", output.description.as_ref())?;
                require_output_field(id, "genre", output.genre.as_ref())?;
                require_output_field(id, "public", output.public.as_ref())?;
                require_output_field(id, "bitrate", output.bitrate.as_ref())?;
                if !matches!(info.container, ContainerKind::Ogg | ContainerKind::Mpeg) {
                    anyhow::bail!(
                        "output '{}' requires Ogg/MPEG container (codec_id '{}')",
                        id,
                        codec_id
                    );
                }
            }
            "srt_out" => {
                require_output_field(id, "target", output.target.as_ref())?;
                require_output_field(id, "latency_ms", output.latency_ms.as_ref())?;
                if matches!(info.container, ContainerKind::Rtp) {
                    anyhow::bail!(
                        "output '{}' does not accept RTP container (codec_id '{}')",
                        id,
                        codec_id
                    );
                }
            }
            "udp_out" => {
                require_output_field(id, "target", output.target.as_ref())?;
                if !matches!(info.container, ContainerKind::Raw | ContainerKind::Rtp) {
                    anyhow::bail!(
                        "output '{}' expects raw/rtp containers (codec_id '{}')",
                        id,
                        codec_id
                    );
                }
            }
            "recorder" => {
                require_output_field(id, "wav_dir", output.wav_dir.as_ref())?;
                require_output_field(id, "retention_days", output.retention_days.as_ref())?;
                if matches!(info.container, ContainerKind::Rtp) {
                    anyhow::bail!(
                        "output '{}' does not support RTP container (codec_id '{}')",
                        id,
                        codec_id
                    );
                }
            }
            other => {
                anyhow::bail!("output '{}' has unsupported type '{}'", id, other);
            }
        }

    }
    Ok(())
}

fn validate_services(
    services: &BTreeMap<String, ServiceConfig>,
    ringbuffer_id: Option<&str>,
    inputs: &BTreeMap<String, InputConfig>,
) -> anyhow::Result<()> {
    let Some(ringbuffer_id) = ringbuffer_id else {
        if services.is_empty() {
            return Ok(());
        }
        anyhow::bail!(
            "services require a ringbuffer; enable monitoring or parallel consumers to use ringbuffers"
        );
    };
    let mut seen_types = BTreeMap::new();
    for (id, service) in services {
        if let Some(buffer) = service.buffer.as_deref() {
            if buffer != ringbuffer_id {
                anyhow::bail!(
                    "service '{}' references unknown ringbuffer '{}'",
                    id,
                    buffer
                );
            }
        }

        if let Some(input_id) = service.input.as_deref() {
            let input = inputs.get(input_id).ok_or_else(|| {
                anyhow::anyhow!("service '{}' references unknown input '{}'", id, input_id)
            })?;
            if let Some(buffer) = service.buffer.as_deref() {
                if input.buffer != buffer {
                    anyhow::bail!(
                        "service '{}' buffer '{}' does not match input '{}' buffer '{}'",
                        id,
                        buffer,
                        input_id,
                        input.buffer
                    );
                }
            }
        }

        let allow_multiple = matches!(
            service.service_type.as_str(),
            "broadcast_http" | "influx_out"
        );

        match service.service_type.as_str() {
            "audio_http" => {
                require_service_field(id, "buffer", service.buffer.as_ref())?;
                require_service_field(id, "codec_id", service.codec_id.as_ref())?;
            }
            "monitoring" => {}
            "peak_analyzer" => {
                require_service_field(id, "buffer", service.buffer.as_ref())?;
                require_service_field(id, "interval_ms", service.interval_ms.as_ref())?;
            }
            "influx_out" => {
                require_service_field(id, "url", service.url.as_ref())?;
                require_service_field(id, "db", service.db.as_ref())?;
                require_service_field(id, "interval_ms", service.interval_ms.as_ref())?;
            }
            "broadcast_http" => {
                require_service_field(id, "url", service.url.as_ref())?;
                require_service_field(id, "interval_ms", service.interval_ms.as_ref())?;
            }
            other => {
                anyhow::bail!("service '{}' has unsupported type '{}'", id, other);
            }
        }

        if !allow_multiple
            && seen_types
                .insert(service.service_type.clone(), id)
                .is_some()
        {
            anyhow::bail!(
                "multiple services of type '{}' are not supported",
                service.service_type
            );
        }
    }
    Ok(())
}

fn require_output_field<T>(id: &str, field: &str, value: Option<&T>) -> anyhow::Result<()> {
    if value.is_none() {
        anyhow::bail!("output '{}' requires {}", id, field);
    }
    Ok(())
}

fn require_service_field<T>(id: &str, field: &str, value: Option<&T>) -> anyhow::Result<()> {
    if value.is_none() {
        anyhow::bail!("service '{}' requires {}", id, field);
    }
    Ok(())
}

fn codec_info_from_config(config: &CodecInstanceConfig) -> CodecInfo {
    CodecInfo {
        kind: codec_kind_from_type(&config.codec_type),
        sample_rate: config.sample_rate.unwrap_or(48_000),
        channels: config.channels.unwrap_or(2),
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

fn ringbuffer_required(
    outputs: &BTreeMap<String, OutputConfig>,
    services: &BTreeMap<String, ServiceConfig>,
) -> bool {
    let monitor_enabled = services.values().any(|service| {
        service.enabled
            && matches!(
                service.service_type.as_str(),
                "monitoring" | "audio_http"
            )
    });

    let consumers = outputs.values().filter(|output| output.enabled).count()
        + services
            .values()
            .filter(|service| {
                service.enabled
                    && matches!(
                        service.service_type.as_str(),
                        "audio_http"
                            | "monitoring"
                            | "peak_analyzer"
                            | "influx_out"
                            | "broadcast_http"
                    )
            })
            .count();

    monitor_enabled || consumers > 1
}

fn default_container_for_type(codec_type: &CodecType) -> ContainerKind {
    match codec_type {
        CodecType::Pcm | CodecType::AacLc | CodecType::Flac => ContainerKind::Raw,
        CodecType::OpusOgg | CodecType::Vorbis => ContainerKind::Ogg,
        CodecType::OpusWebrtc => ContainerKind::Rtp,
        CodecType::Mp3 => ContainerKind::Mpeg,
    }
}
