// src/services/mod.rs - Vereinfacht

use anyhow::Result;
use crate::core::Service;
use std::collections::HashMap;
use std::time::Duration;

/// Erstellt Service basierend auf Konfiguration
pub fn create(_cfg: &crate::config::ServiceConfig) -> Result<Box<dyn Service>> {
    // Für jetzt nur ein Dummy-Service
    Ok(Box::new(DummyService::new()))
}

// ============================================================================
// DUMMY SERVICE (Stub)
// ============================================================================

pub struct DummyService {
    name: String,
    running: bool,
    start_time: Option<std::time::Instant>,
}

impl DummyService {
    pub fn new() -> Self {
        Self {
            name: "dummy".to_string(),
            running: false,
            start_time: None,
        }
    }
}

impl Service for DummyService {
    fn name(&self) -> &str {
        &self.name
    }
    
    fn start(&mut self) -> Result<()> {
        log::info!("DummyService started");
        self.running = true;
        self.start_time = Some(std::time::Instant::now());
        Ok(())
    }
    
    fn stop(&mut self) -> Result<()> {
        log::info!("DummyService stopped");
        self.running = false;
        Ok(())
    }
    
    fn health(&self) -> crate::core::ServiceHealth {
        crate::core::ServiceHealth {
            healthy: self.running,
            uptime: self.start_time.map(|t| t.elapsed()).unwrap_or(Duration::from_secs(0)),
            metrics: HashMap::new(),
        }
    }
}
