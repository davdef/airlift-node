// src/processors/peak.rs - Mutability Problem beheben

use anyhow::Result;
use std::sync::Arc;
use crate::core::{Processor, PcmRingBuffer};

pub struct PeakMonitor {
    buffer: Option<Arc<std::sync::Mutex<PcmRingBuffer>>>,  // Mutex hier!
    name: String,
    running: bool,
}

impl PeakMonitor {
    pub fn new(_cfg: &crate::config::ProcessorConfig) -> Result<Self> {
        let name = "peak_monitor".to_string();
        Ok(Self {
            buffer: None,
            name,
            running: false,
        })
    }
}

impl Processor for PeakMonitor {
    fn name(&self) -> &str {
        &self.name
    }
    
    fn attach(&mut self, buffer: Arc<PcmRingBuffer>) -> Result<()> {
        // Wrap buffer in Mutex
        self.buffer = Some(Arc::new(std::sync::Mutex::new(buffer)));
        self.running = true;
        Ok(())
    }
    
    fn process(&mut self) -> Result<()> {
        if !self.running {
            return Ok(());
        }
        
        if let Some(buffer) = &self.buffer {
            if let Ok(mut buffer) = buffer.lock() {
                let _samples = buffer.read_new_samples(&self.name)?;
                log::debug!("PeakMonitor {}: Processing {} samples", 
                    self.name, _samples.len());
            }
        }
        
        Ok(())
    }
    
    fn detach(&mut self) -> Result<()> {
        self.buffer = None;
        self.running = false;
        Ok(())
    }
}
