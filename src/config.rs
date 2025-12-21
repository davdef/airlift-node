use serde::{Deserialize, Serialize};

// ---------- Ring ----------
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RingConfig {
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

// ---------- MP3 ----------
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Mp3OutConfig {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub mount: String,
    pub user: String,
    pub password: String,
    pub name: String,
    pub description: String,
    pub genre: String,
    pub public: bool,
    pub bitrate: u32,
    #[serde(default)]
    pub codec_id: Option<String>,
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

// ---------- Recorder ----------
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct HourlyRecorderConfig {
    pub enabled: bool,
    pub base_dir: String,
}

// ---------- Monitoring ----------
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct MonitoringConfig {
    pub http_port: u16,
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
    pub ring: RingConfig,
    pub alsa_in: Option<AlsaInConfig>,
    pub udp_out: Option<UdpOutConfig>,
    pub icecast_out: Option<IcecastOutConfig>,
    pub mp3_out: Option<Mp3OutConfig>,
    pub srt_in: Option<SrtInConfig>,
    pub srt_out: Option<SrtOutConfig>,
    pub recorder: Option<RecorderConfigToml>,
    #[serde(default)]
    pub monitoring: MonitoringConfig,
    #[serde(default)]
    pub metadata: MetadataConfig,
    pub influx_history: Option<InfluxHistoryConfig>,
    #[serde(default)]
    pub codecs: Vec<CodecInstanceConfig>,
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
