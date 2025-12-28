// src/core/buffer_registry.rs
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use super::ringbuffer::AudioRingBuffer;
use anyhow::Result;

#[derive(Clone)]
pub struct BufferRegistry {
    buffers: Arc<RwLock<HashMap<String, Arc<AudioRingBuffer>>>>,
}

impl BufferRegistry {
    pub fn new() -> Self {
        Self {
            buffers: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    /// Registriere einen Buffer unter einem Namen
    pub fn register(&self, name: &str, buffer: Arc<AudioRingBuffer>) -> Result<()> {
        let mut buffers = self.buffers.write()
            .map_err(|e| anyhow::anyhow!("Failed to acquire write lock: {}", e))?;
        
        if buffers.contains_key(name) {
            anyhow::bail!("Buffer '{}' already registered", name);
        }
        
        buffers.insert(name.to_string(), buffer);
        log::debug!("Registered buffer '{}'", name);
        Ok(())
    }
    
    /// Aktualisiere einen Buffer (überschreibt falls existiert)
    pub fn update(&self, name: &str, buffer: Arc<AudioRingBuffer>) -> Result<()> {
        let mut buffers = self.buffers.write()
            .map_err(|e| anyhow::anyhow!("Failed to acquire write lock: {}", e))?;
        
        buffers.insert(name.to_string(), buffer);
        log::debug!("Updated buffer '{}'", name);
        Ok(())
    }
    
    /// Hole einen Buffer
    pub fn get(&self, name: &str) -> Option<Arc<AudioRingBuffer>> {
        let buffers = self.buffers.read().ok()?;
        buffers.get(name).cloned()
    }
    
    /// Entferne einen Buffer
    pub fn remove(&self, name: &str) -> Result<()> {
        let mut buffers = self.buffers.write()
            .map_err(|e| anyhow::anyhow!("Failed to acquire write lock: {}", e))?;
        
        if buffers.remove(name).is_some() {
            log::debug!("Removed buffer '{}'", name);
            Ok(())
        } else {
            anyhow::bail!("Buffer '{}' not found", name)
        }
    }
    
    /// Liste aller registrierten Buffer-Namen
    pub fn list(&self) -> Vec<String> {
        let buffers = match self.buffers.read() {
            Ok(guard) => guard,
            Err(poisoned) => {
                log::error!("Buffer registry lock poisoned, recovering");
                poisoned.into_inner()
            }
        };
        
        buffers.keys().cloned().collect()
    }
    
    /// Prüfe ob Buffer existiert
    pub fn exists(&self, name: &str) -> bool {
        let buffers = match self.buffers.read() {
            Ok(guard) => guard,
            Err(poisoned) => {
                log::error!("Buffer registry lock poisoned, recovering");
                poisoned.into_inner()
            }
        };
        
        buffers.contains_key(name)
    }
}
