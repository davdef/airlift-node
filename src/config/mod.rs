use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;

use anyhow::{bail, Context};

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
pub struct MonitoringConfig {
    pub http_port: u16,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub node_name: String,
    pub producers: HashMap<String, ProducerConfig>,
    pub processors: HashMap<String, ProcessorConfig>,
    pub consumers: HashMap<String, ConsumerConfig>,
    pub flows: HashMap<String, FlowConfig>,
    #[serde(default)]
    pub monitoring: MonitoringConfig,
}

impl Config {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let content = fs::read_to_string(path)?;
        let config: Self = toml::from_str(&content)?;
        config.validate().context("config validation failed")?;
        Ok(config)
    }
    
    pub fn save(&self, path: &str) -> anyhow::Result<()> {
        let content = toml::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.node_name.trim().is_empty() {
            bail!("node_name must not be empty");
        }

        for (name, producer) in &self.producers {
            producer.validate(name)?;
        }

        for (name, processor) in &self.processors {
            processor.validate(name)?;
        }

        for (name, consumer) in &self.consumers {
            consumer.validate(name)?;
        }

        for (name, flow) in &self.flows {
            flow.validate(name)?;
            for input in &flow.inputs {
                if !self.producers.contains_key(input) {
                    bail!("flow '{}' references missing producer '{}'", name, input);
                }
            }
            for processor in &flow.processors {
                if !self.processors.contains_key(processor) {
                    bail!("flow '{}' references missing processor '{}'", name, processor);
                }
            }
            for output in &flow.outputs {
                if !self.consumers.contains_key(output) {
                    bail!("flow '{}' references missing consumer '{}'", name, output);
                }
            }
        }

        if self.monitoring.http_port == 0 {
            bail!("monitoring.http_port must be > 0");
        }

        Ok(())
    }

    pub fn apply_patch(&mut self, patch: &ConfigPatch) -> anyhow::Result<()> {
        let mut next = self.clone();
        patch.apply_to(&mut next)?;
        next.validate()?;
        *self = next;
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
            monitoring: MonitoringConfig::default(),
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

impl ProducerConfig {
    fn validate(&self, name: &str) -> anyhow::Result<()> {
        if name.trim().is_empty() {
            bail!("producer name must not be empty");
        }
        if self.producer_type.trim().is_empty() {
            bail!("producer '{}' type must not be empty", name);
        }
        if let Some(ref device) = self.device {
            if device.trim().is_empty() {
                bail!("producer '{}' device must not be empty", name);
            }
        }
        if let Some(ref path) = self.path {
            if path.trim().is_empty() {
                bail!("producer '{}' path must not be empty", name);
            }
        }
        if let Some(channels) = self.channels {
            if channels == 0 {
                bail!("producer '{}' channels must be > 0", name);
            }
        }
        if let Some(sample_rate) = self.sample_rate {
            if sample_rate == 0 {
                bail!("producer '{}' sample_rate must be > 0", name);
            }
        }
        Ok(())
    }
}

impl ProcessorConfig {
    fn validate(&self, name: &str) -> anyhow::Result<()> {
        if name.trim().is_empty() {
            bail!("processor name must not be empty");
        }
        if self.processor_type.trim().is_empty() {
            bail!("processor '{}' type must not be empty", name);
        }
        Ok(())
    }
}

impl ConsumerConfig {
    fn validate(&self, name: &str) -> anyhow::Result<()> {
        if name.trim().is_empty() {
            bail!("consumer name must not be empty");
        }
        if self.consumer_type.trim().is_empty() {
            bail!("consumer '{}' type must not be empty", name);
        }
        if let Some(ref path) = self.path {
            if path.trim().is_empty() {
                bail!("consumer '{}' path must not be empty", name);
            }
        }
        if let Some(ref url) = self.url {
            if url.trim().is_empty() {
                bail!("consumer '{}' url must not be empty", name);
            }
        }
        Ok(())
    }
}

impl FlowConfig {
    fn validate(&self, name: &str) -> anyhow::Result<()> {
        if name.trim().is_empty() {
            bail!("flow name must not be empty");
        }
        for input in &self.inputs {
            if input.trim().is_empty() {
                bail!("flow '{}' has empty input reference", name);
            }
        }
        for processor in &self.processors {
            if processor.trim().is_empty() {
                bail!("flow '{}' has empty processor reference", name);
            }
        }
        for output in &self.outputs {
            if output.trim().is_empty() {
                bail!("flow '{}' has empty output reference", name);
            }
        }
        Ok(())
    }
}

impl Default for MonitoringConfig {
    fn default() -> Self {
        Self { http_port: 8087 }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ConfigPatch {
    pub node_name: Option<String>,
    pub producers: Option<HashMap<String, ProducerConfigPatch>>,
    pub processors: Option<HashMap<String, ProcessorConfigPatch>>,
    pub consumers: Option<HashMap<String, ConsumerConfigPatch>>,
    pub flows: Option<HashMap<String, FlowConfigPatch>>,
    pub monitoring: Option<MonitoringConfigPatch>,
}

impl ConfigPatch {
    fn apply_to(&self, config: &mut Config) -> anyhow::Result<()> {
        if let Some(ref node_name) = self.node_name {
            if node_name.trim().is_empty() {
                bail!("node_name must not be empty");
            }
            config.node_name = node_name.clone();
        }

        if let Some(ref producers) = self.producers {
            for (name, patch) in producers {
                let mut next = config
                    .producers
                    .get(name)
                    .cloned()
                    .unwrap_or_else(ProducerConfig::default);
                patch.apply_to(&mut next)?;
                next.validate(name)?;
                config.producers.insert(name.clone(), next);
            }
        }

        if let Some(ref processors) = self.processors {
            for (name, patch) in processors {
                let mut next = config
                    .processors
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| ProcessorConfig {
                        processor_type: "unknown".to_string(),
                        enabled: true,
                        config: HashMap::new(),
                    });
                patch.apply_to(&mut next)?;
                next.validate(name)?;
                config.processors.insert(name.clone(), next);
            }
        }

        if let Some(ref consumers) = self.consumers {
            for (name, patch) in consumers {
                let mut next = config
                    .consumers
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| ConsumerConfig {
                        consumer_type: "file".to_string(),
                        enabled: true,
                        path: None,
                        url: None,
                        config: HashMap::new(),
                    });
                patch.apply_to(&mut next)?;
                next.validate(name)?;
                config.consumers.insert(name.clone(), next);
            }
        }

        if let Some(ref flows) = self.flows {
            for (name, patch) in flows {
                let mut next = config
                    .flows
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| FlowConfig {
                        enabled: true,
                        inputs: Vec::new(),
                        processors: Vec::new(),
                        outputs: Vec::new(),
                        config: HashMap::new(),
                    });
                patch.apply_to(&mut next)?;
                next.validate(name)?;
                config.flows.insert(name.clone(), next);
            }
        }

        if let Some(ref monitoring) = self.monitoring {
            monitoring.apply_to(&mut config.monitoring)?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct MonitoringConfigPatch {
    pub http_port: Option<u16>,
}

impl MonitoringConfigPatch {
    fn apply_to(&self, target: &mut MonitoringConfig) -> anyhow::Result<()> {
        if let Some(port) = self.http_port {
            if port == 0 {
                bail!("monitoring.http_port must be > 0");
            }
            target.http_port = port;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ProducerConfigPatch {
    #[serde(rename = "type")]
    pub producer_type: Option<String>,
    pub enabled: Option<bool>,
    pub device: Option<String>,
    pub path: Option<String>,
    pub channels: Option<u8>,
    pub sample_rate: Option<u32>,
    pub loop_audio: Option<bool>,
    pub config: Option<HashMap<String, serde_json::Value>>,
}

impl ProducerConfigPatch {
    fn apply_to(&self, target: &mut ProducerConfig) -> anyhow::Result<()> {
        if let Some(ref producer_type) = self.producer_type {
            if producer_type.trim().is_empty() {
                bail!("producer type must not be empty");
            }
            target.producer_type = producer_type.clone();
        }
        if let Some(enabled) = self.enabled {
            target.enabled = enabled;
        }
        if let Some(ref device) = self.device {
            target.device = Some(device.clone());
        }
        if let Some(ref path) = self.path {
            target.path = Some(path.clone());
        }
        if let Some(channels) = self.channels {
            target.channels = Some(channels);
        }
        if let Some(sample_rate) = self.sample_rate {
            target.sample_rate = Some(sample_rate);
        }
        if let Some(loop_audio) = self.loop_audio {
            target.loop_audio = Some(loop_audio);
        }
        if let Some(ref config) = self.config {
            target.config.extend(config.clone());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ProcessorConfigPatch {
    #[serde(rename = "type")]
    pub processor_type: Option<String>,
    pub enabled: Option<bool>,
    pub config: Option<HashMap<String, serde_json::Value>>,
}

impl ProcessorConfigPatch {
    fn apply_to(&self, target: &mut ProcessorConfig) -> anyhow::Result<()> {
        if let Some(ref processor_type) = self.processor_type {
            if processor_type.trim().is_empty() {
                bail!("processor type must not be empty");
            }
            target.processor_type = processor_type.clone();
        }
        if let Some(enabled) = self.enabled {
            target.enabled = enabled;
        }
        if let Some(ref config) = self.config {
            target.config.extend(config.clone());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ConsumerConfigPatch {
    #[serde(rename = "type")]
    pub consumer_type: Option<String>,
    pub enabled: Option<bool>,
    pub path: Option<String>,
    pub url: Option<String>,
    pub config: Option<HashMap<String, serde_json::Value>>,
}

impl ConsumerConfigPatch {
    fn apply_to(&self, target: &mut ConsumerConfig) -> anyhow::Result<()> {
        if let Some(ref consumer_type) = self.consumer_type {
            if consumer_type.trim().is_empty() {
                bail!("consumer type must not be empty");
            }
            target.consumer_type = consumer_type.clone();
        }
        if let Some(enabled) = self.enabled {
            target.enabled = enabled;
        }
        if let Some(ref path) = self.path {
            target.path = Some(path.clone());
        }
        if let Some(ref url) = self.url {
            target.url = Some(url.clone());
        }
        if let Some(ref config) = self.config {
            target.config.extend(config.clone());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct FlowConfigPatch {
    pub enabled: Option<bool>,
    pub inputs: Option<Vec<String>>,
    pub processors: Option<Vec<String>>,
    pub outputs: Option<Vec<String>>,
    pub config: Option<HashMap<String, serde_json::Value>>,
}

impl FlowConfigPatch {
    fn apply_to(&self, target: &mut FlowConfig) -> anyhow::Result<()> {
        if let Some(enabled) = self.enabled {
            target.enabled = enabled;
        }
        if let Some(ref inputs) = self.inputs {
            target.inputs = inputs.clone();
        }
        if let Some(ref processors) = self.processors {
            target.processors = processors.clone();
        }
        if let Some(ref outputs) = self.outputs {
            target.outputs = outputs.clone();
        }
        if let Some(ref config) = self.config {
            target.config.extend(config.clone());
        }
        Ok(())
    }
}
