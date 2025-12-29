use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;

use crate::core::processor::Processor;

#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub description: String,
}

impl PluginInfo {
    pub fn new(name: impl Into<String>, version: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            description: description.into(),
        }
    }
}

pub trait AudioPlugin: Send + Sync {
    fn plugin_info(&self) -> PluginInfo;
    fn create(&self, config: Value) -> Result<Box<dyn Processor>>;
}

pub type PluginFactory = Arc<dyn AudioPlugin>;

pub struct ProcessorPluginAdapter<F>
where
    F: Fn(Value) -> Result<Box<dyn Processor>> + Send + Sync + 'static,
{
    info: PluginInfo,
    factory: F,
}

impl<F> ProcessorPluginAdapter<F>
where
    F: Fn(Value) -> Result<Box<dyn Processor>> + Send + Sync + 'static,
{
    pub fn new(info: PluginInfo, factory: F) -> Self {
        Self { info, factory }
    }
}

impl<F> AudioPlugin for ProcessorPluginAdapter<F>
where
    F: Fn(Value) -> Result<Box<dyn Processor>> + Send + Sync + 'static,
{
    fn plugin_info(&self) -> PluginInfo {
        self.info.clone()
    }

    fn create(&self, config: Value) -> Result<Box<dyn Processor>> {
        (self.factory)(config)
    }
}
