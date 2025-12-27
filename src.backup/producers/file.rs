// src/producers/file.rs
use anyhow::Result;
use crate::core::{Producer, AudioFormat, ProducerStatus, PcmRingBuffer, SampleType};
use std::sync::Arc;

pub struct FileProducer {
    buffer: Option<Arc<PcmRingBuffer>>,
    format: AudioFormat,
    running: bool,
}

impl FileProducer {
    pub fn new(_cfg: &crate::config::ProducerConfig) -> Result<Self> {
        Ok(Self {
            buffer: None,
            format: AudioFormat {
                sample_rate: 48000,
                channels: 2,
                sample_type: SampleType::F32,
            },
            running: false,
        })
    }
}

impl Producer for FileProducer {
    fn start(&mut self) -> Result<()> {
        log::info!("FileProducer started");
        self.running = true;
        Ok(())
    }
    
    fn stop(&mut self) -> Result<()> {
        log::info!("FileProducer stopped");
        self.running = false;
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
            running: self.running,
            connected: self.buffer.is_some(),
            samples_written: 0,
            errors: 0,
        }
    }
}
