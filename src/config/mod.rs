use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProducerConfig {
    #[serde(rename = "type")]
    pub producer_type: String,
    pub enabled: bool,
    pub device: Option<String>,
    pub path: Option<String>,
    pub channels: Option<u8>,
    pub sample_rate: Option<u32>,
    pub loop_audio: Option<bool>,
    #[serde(default)]
    pub config: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProcessorConfig {
    #[serde(rename = "type")]
    pub processor_type: String,
    pub enabled: bool,
    #[serde(default)]
    pub config: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConsumerConfig {
    #[serde(rename = "type")]
    pub consumer_type: String,
    pub enabled: bool,
    pub path: Option<String>,
    pub url: Option<String>,
    #[serde(default)]
    pub config: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FlowConfig {
    pub enabled: bool,
    pub inputs: Vec<String>,
    pub processors: Vec<String>,
    pub outputs: Vec<String>,
    
    #[serde(default)]
    pub config: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub node_name: String,
    pub producers: HashMap<String, ProducerConfig>,
    pub processors: HashMap<String, ProcessorConfig>,
    pub consumers: HashMap<String, ConsumerConfig>,
    pub flows: HashMap<String, FlowConfig>,
}

impl Config {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let content = fs::read_to_string(path)?;
        Ok(toml::from_str(&content)?)
    }
    
    pub fn save(&self, path: &str) -> anyhow::Result<()> {
        let content = toml::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            node_name: "airlift-node".to_string(),
            producers: HashMap::new(),
            processors: HashMap::new(),
            consumers: HashMap::new(),
            flows: HashMap::new(),
        }
    }
}

impl Default for ProducerConfig {
    fn default() -> Self {
        Self {
            producer_type: "file".to_string(),
            enabled: true,
            device: None,
            path: None,
            channels: Some(2),
            sample_rate: Some(48000),
            loop_audio: Some(false),
            config: HashMap::new(), // ← Wichtig!
        }
    }
}
