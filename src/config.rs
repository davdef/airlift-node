use serde::Deserialize;

// ---------- Ring ----------
#[derive(Debug, Deserialize, Clone)]
pub struct RingConfig {
    pub slots: usize,
    pub prealloc_samples: usize,
}

// ---------- ALSA ----------
#[derive(Debug, Deserialize, Clone)]
pub struct AlsaInConfig {
    pub enabled: bool,
    pub device: String,
}

// ---------- UDP ----------
#[derive(Debug, Deserialize, Clone)]
pub struct UdpOutConfig {
    pub enabled: bool,
    pub target: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SrtOutConfig {
    pub enabled: bool,
    pub target: String, // "host:port"
    pub latency_ms: u32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RecorderConfigToml {
    pub enabled: bool,
    pub wav_dir: String,
    pub retention_days: u64,
    pub mp3: Option<Mp3ConfigToml>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Mp3ConfigToml {
    pub dir: String,
    pub bitrate: u32,
}

// ---------- Icecast (Opus) ----------
#[derive(Debug, Deserialize, Clone)]
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
}

// ---------- MP3 ----------
#[derive(Debug, Deserialize, Clone)]
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
}

// ---------- Recorder ----------
#[derive(Debug, Deserialize, Clone)]
pub struct HourlyRecorderConfig {
    pub enabled: bool,
    pub base_dir: String,
}

// ---------- Monitoring ----------
#[derive(Debug, Deserialize, Clone, Default)]
pub struct MonitoringConfig {
    pub http_port: u16,
}

// ---------- Metadata ----------
#[derive(Debug, Deserialize, Clone, Default)]
pub struct MetadataConfig {
    pub default: String,
}

// ---------- Influx History ----------
#[derive(Debug, Deserialize, Clone)]
pub struct InfluxHistoryConfig {
    pub enabled: bool,
    pub base_url: String,
    pub token: String,
    pub org: String,
    pub bucket: String,
}

// ---------- Root ----------
#[derive(Debug, Deserialize, Clone)]
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
}

#[derive(Debug, Deserialize, Clone)]
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
