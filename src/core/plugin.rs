use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::sync::Arc;

use crate::core::processor::{basic, Processor};
use crate::processors::Mixer;

pub trait AudioPlugin: Send + Sync {
    fn create(&self, config: serde_json::Value) -> Result<Box<dyn Processor>>;
}

pub struct PluginRegistry {
    plugins: HashMap<String, Arc<dyn AudioPlugin>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
        }
    }

    pub fn register(&mut self, name: &str, plugin: Arc<dyn AudioPlugin>) {
        self.plugins.insert(name.to_string(), plugin);
    }

    pub fn create(&self, name: &str, config: serde_json::Value) -> Result<Box<dyn Processor>> {
        let plugin = self
            .plugins
            .get(name)
            .ok_or_else(|| anyhow!("Unknown plugin: {}", name))?;
        plugin.create(config)
    }
}

pub fn register_builtin_plugins(registry: &mut PluginRegistry) {
    registry.register("passthrough", Arc::new(PassthroughPlugin));
    registry.register("gain", Arc::new(GainPlugin));
    registry.register("mixer", Arc::new(MixerPlugin));
}

struct PassthroughPlugin;

impl AudioPlugin for PassthroughPlugin {
    fn create(&self, config: serde_json::Value) -> Result<Box<dyn Processor>> {
        let name = plugin_name(&config)?;
        Ok(Box::new(basic::PassThrough::new(&name)))
    }
}

struct GainPlugin;

impl AudioPlugin for GainPlugin {
    fn create(&self, config: serde_json::Value) -> Result<Box<dyn Processor>> {
        let name = plugin_name(&config)?;
        let gain = config
            .get("gain")
            .and_then(|value| value.as_f64())
            .unwrap_or(1.0) as f32;
        Ok(Box::new(basic::Gain::new(&name, gain)))
    }
}

struct MixerPlugin;

impl AudioPlugin for MixerPlugin {
    fn create(&self, config: serde_json::Value) -> Result<Box<dyn Processor>> {
        let name = plugin_name(&config)?;
        let mut mixer = Mixer::new(&name);
        mixer.update_config(config)?;
        Ok(Box::new(mixer))
    }
}

fn plugin_name(config: &serde_json::Value) -> Result<String> {
    config
        .get("name")
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
        .ok_or_else(|| anyhow!("Plugin config missing 'name'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_missing_plugin_error() {
        let registry = PluginRegistry::new();
        let error = registry.create("missing", serde_json::json!({})).unwrap_err();
        assert!(error.to_string().contains("Unknown plugin"));
    }
}
