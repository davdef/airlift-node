use std::collections::HashMap;

use crate::config;
use crate::core::processor::Processor;
use crate::processors;

type ProcessorFactory =
    Box<dyn Fn(&str, &config::ProcessorConfig) -> anyhow::Result<Box<dyn Processor>> + Send + Sync>;

pub struct PluginRegistry {
    processors: HashMap<String, ProcessorFactory>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            processors: HashMap::new(),
        }
    }

    pub fn register_processor<F>(&mut self, processor_type: impl Into<String>, factory: F)
    where
        F: Fn(&str, &config::ProcessorConfig) -> anyhow::Result<Box<dyn Processor>>
            + Send
            + Sync
            + 'static,
    {
        self.processors
            .insert(processor_type.into(), Box::new(factory));
    }

    pub fn register_default_plugins(&mut self) {
        self.register_processor("passthrough", |name, _cfg| {
            Ok(Box::new(crate::core::processor::basic::PassThrough::new(
                name,
            )))
        });

        self.register_processor("gain", |name, cfg| {
            let gain = cfg
                .config
                .get("gain")
                .and_then(|v| v.as_f64())
                .unwrap_or(1.0) as f32;
            Ok(Box::new(crate::core::processor::basic::Gain::new(
                name, gain,
            )))
        });

self.register_processor("mixer", |name, cfg| {
    let mut mixer = processors::Mixer::new(name);

let mixer_cfg: processors::mixer::MixerConfig =
    serde_json::from_value(serde_json::Value::Object(
        cfg.config.clone().into_iter().collect()
    ))
    .map_err(|e| anyhow::anyhow!("invalid mixer config: {}", e))?;

    mixer.update_config(&mixer_cfg)?;
    Ok(Box::new(mixer))
});

    }

    pub fn create_processor(
        &self,
        processor_name: &str,
        processor_cfg: &config::ProcessorConfig,
    ) -> anyhow::Result<Box<dyn Processor>> {
        let processor_type = processor_cfg.processor_type.as_str();
        let factory = self.processors.get(processor_type).ok_or_else(|| {
            anyhow::anyhow!("Unknown processor type '{}'", processor_cfg.processor_type)
        })?;
        factory(processor_name, processor_cfg)
    }
}

pub fn build_plugin_registry() -> PluginRegistry {
    let mut registry = PluginRegistry::new();
    registry.register_default_plugins();
    registry
}
