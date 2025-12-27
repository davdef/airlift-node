// src/processors/mod.rs

use anyhow::Result;
use crate::core::Processor;

pub mod peak;

/// Erstellt Processor basierend auf Konfiguration
pub fn create(cfg: &crate::config::ProcessorConfig) -> Result<Box<dyn Processor>> {
    match cfg.r#type.as_str() {
        "peak_monitor" => {
            let processor = peak::PeakMonitor::new(cfg)?;
            Ok(Box::new(processor))  // Direkt PeakMonitor, kein extra Struct
        },
        _ => Err(anyhow::anyhow!("Unknown processor type: {}", cfg.r#type)),
    }
}
