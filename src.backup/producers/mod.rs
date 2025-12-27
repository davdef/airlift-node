// src/producers/mod.rs

use anyhow::Result;
use crate::core::{Producer, AudioFormat, ProducerStatus, PcmRingBuffer};
use std::sync::Arc;

pub mod file;
pub mod alsa;

/// Erstellt Producer basierend auf Konfiguration
pub fn create(cfg: &crate::config::ProducerConfig) -> Result<Box<dyn Producer>> {
    match cfg.r#type.as_str() {
        "file" => Ok(Box::new(file::FileProducer::new(cfg)?)),
        "alsa" => Ok(Box::new(alsa::AlsaProducer::new(cfg)?)),
        _ => Err(anyhow::anyhow!("Unknown producer type: {}", cfg.r#type)),
    }
}

// ============================================================================
// FILE PRODUCER (Stub)
// ============================================================================

pub struct FileProducer {
    buffer: Option<Arc<PcmRingBuffer>>,
    format: AudioFormat,
}

impl FileProducer {
    pub fn new(_cfg: &crate::config::ProducerConfig) -> Result<Self> {
        Ok(Self {
            buffer: None,
            format: AudioFormat {
                sample_rate: 48000,
                channels: 2,
                sample_type: crate::core::SampleType::F32,
            },
        })
    }
}

impl Producer for FileProducer {
    fn start(&mut self) -> Result<()> {
        log::info!("FileProducer started");
        Ok(())
    }
    
    fn stop(&mut self) -> Result<()> {
        log::info!("FileProducer stopped");
        Ok(())
    }
    
    fn format(&self) -> AudioFormat {
        self.format
    }
    
    fn attach_buffer(&mut self, buffer: Arc<PcmRingBuffer>) -> Result<()> {
        self.buffer = Some(buffer);
        Ok(())
    }
    
    fn status(&self) -> ProducerStatus {
        ProducerStatus {
            running: true,
            connected: self.buffer.is_some(),
            samples_written: 0,
            errors: 0,
        }
    }
}

// ============================================================================
// ALSA PRODUCER (Stub)
// ============================================================================

pub struct AlsaProducer {
    buffer: Option<Arc<PcmRingBuffer>>,
    format: AudioFormat,
}

impl AlsaProducer {
    pub fn new(_cfg: &crate::config::ProducerConfig) -> Result<Self> {
        Ok(Self {
            buffer: None,
            format: AudioFormat {
                sample_rate: 48000,
                channels: 2,
                sample_type: crate::core::SampleType::I16,
            },
        })
    }
}

impl Producer for AlsaProducer {
    fn start(&mut self) -> Result<()> {
        log::info!("AlsaProducer started");
        Ok(())
    }
    
    fn stop(&mut self) -> Result<()> {
        log::info!("AlsaProducer stopped");
        Ok(())
    }
    
    fn format(&self) -> AudioFormat {
        self.format
    }
    
    fn attach_buffer(&mut self, buffer: Arc<PcmRingBuffer>) -> Result<()> {
        self.buffer = Some(buffer);
        Ok(())
    }
    
    fn status(&self) -> ProducerStatus {
        ProducerStatus {
            running: true,
            connected: self.buffer.is_some(),
            samples_written: 0,
            errors: 0,
        }
    }
}
