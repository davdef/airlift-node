use crate::core::processor::{Processor, ProcessorStatus};
use crate::core::ringbuffer::{AudioRingBuffer, PcmFrame};
use crate::core::logging::{ComponentLogger, LogContext};
use crate::core::BufferRegistry;
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MixerInputConfig {
    pub name: String,           // Interne Name im Mixer (z.B. "mic", "system")
    pub source: String,         // Buffer-Name in der Registry (z.B. "alsa_input", "file_output")
    pub gain: f32,              // Gain für diesen Input (0.0 - 1.0 oder mehr)
    pub enabled: Option<bool>,  // Optional: Input deaktivieren
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MixerConfig {
    pub inputs: Vec<MixerInputConfig>,
    pub output_sample_rate: Option<u32>,
    pub output_channels: Option<u8>,
    pub master_gain: Option<f32>,
    pub auto_connect: Option<bool>,  // Automatisch Buffers aus Registry verbinden
}

pub struct Mixer {
    name: String,
    config: MixerConfig,
    input_buffers: Vec<(String, f32, Arc<AudioRingBuffer>)>, // (source_name, gain, buffer)
    output_sample_rate: u32,
    output_channels: u8,
    master_gain: f32,
    buffer_registry: Option<Arc<BufferRegistry>>,
    connected: bool,
}

impl Mixer {
    pub fn new(name: &str) -> Self {
        let config = MixerConfig {
            inputs: Vec::new(),
            output_sample_rate: None,
            output_channels: None,
            master_gain: None,
            auto_connect: Some(true),
        };
        
        Self {
            name: name.to_string(),
            config,
            input_buffers: Vec::new(),
            output_sample_rate: 48000,
            output_channels: 2,
            master_gain: 1.0,
            buffer_registry: None,
            connected: false,
        }
    }
    
    pub fn from_config(name: &str, config: MixerConfig) -> Self {
        let inputs_len = config.inputs.len();
        let output_sample_rate = config.output_sample_rate.unwrap_or(48000);
        let output_channels = config.output_channels.unwrap_or(2);
        let master_gain = config.master_gain.unwrap_or(1.0);
        
        let mixer = Self {
            name: name.to_string(),
            config,
            input_buffers: Vec::new(),
            output_sample_rate,
            output_channels,
            master_gain,
            buffer_registry: None,
            connected: false,
        };
        
        mixer.info(&format!("Created mixer '{}' with {} inputs", name, inputs_len));
        mixer
    }
    
    /// Setze die Buffer-Registry für automatisches Verbinden
    pub fn set_buffer_registry(&mut self, registry: Arc<BufferRegistry>) {
        self.buffer_registry = Some(registry);
        self.info("Buffer registry set");
    }
    
    /// Verbinde Mixer-Inputs mit Buffers aus der Registry
    pub fn connect_from_registry(&mut self) -> Result<()> {
        if let Some(registry) = &self.buffer_registry {
            self.input_buffers.clear();
            let mut connection_errors = Vec::new();
            
            for input_config in &self.config.inputs {
                // Prüfe ob Input enabled ist (default: true)
                if let Some(false) = input_config.enabled {
                    self.debug(&format!("Input '{}' disabled, skipping", input_config.name));
                    continue;
                }
                
                if let Some(buffer) = registry.get(&input_config.source) {
                    self.input_buffers.push((
                        input_config.source.clone(),
                        input_config.gain,
                        buffer,
                    ));
                    
                    self.info(&format!(
                        "Connected input '{}' to source '{}' (gain: {})",
                        input_config.name, input_config.source, input_config.gain
                    ));
                } else {
                    let error = format!(
                        "Source '{}' not found in registry for input '{}'",
                        input_config.source, input_config.name
                    );
                    connection_errors.push(error.clone());
                    self.error(&error);
                }
            }
            
            self.connected = connection_errors.is_empty() || !self.input_buffers.is_empty();
            
            if !connection_errors.is_empty() {
                self.warn(&format!(
                    "Mixer connected with {} error(s), {} input(s) active",
                    connection_errors.len(), self.input_buffers.len()
                ));
                bail!("Failed to connect some inputs: {:?}", connection_errors);
            } else {
                self.info(&format!("Mixer fully connected with {} inputs", self.input_buffers.len()));
                Ok(())
            }
        } else {
            self.error("No buffer registry set");
            bail!("Buffer registry not set")
        }
    }
    
    /// Manuell einen Input verbinden (überschreibt Config)
    pub fn connect_input(&mut self, source_name: &str, gain: f32, buffer: Arc<AudioRingBuffer>) {
        // Entferne existierenden Eintrag für diese Source
        self.input_buffers.retain(|(name, _, _)| name != source_name);
        
        self.input_buffers.push((source_name.to_string(), gain, buffer));
        self.connected = true;
        
        self.info(&format!("Manually connected source '{}' (gain: {})", source_name, gain));
    }
    
    /// Aktualisiere Mixer-Konfiguration zur Laufzeit
pub fn update_config(&mut self, config: &MixerConfig) -> Result<()> {  // &MixerConfig
    self.config = config.clone();  // Clone
    
    self.output_sample_rate = config.output_sample_rate.unwrap_or(self.output_sample_rate);
    self.output_channels = config.output_channels.unwrap_or(self.output_channels);
    self.master_gain = config.master_gain.unwrap_or(self.master_gain);
    
    self.info(&format!("Updated mixer config with {} inputs", config.inputs.len()));
        
        // Bei auto_connect neu verbinden
        if config.auto_connect.unwrap_or(true) {
            if self.buffer_registry.is_some() {
                self.connect_from_registry()?;
            } else {
                self.warn("Cannot auto-connect: no buffer registry set");
            }
        } else {
            self.info("Auto-connect disabled, inputs must be manually connected");
            self.input_buffers.clear();
            self.connected = false;
        }
        
        Ok(())
    }
    
    /// Mixing-Logik
    fn mix_frame(&self) -> Option<PcmFrame> {
        if !self.connected || self.input_buffers.is_empty() {
            return None;
        }
        
        let target_samples = (self.output_sample_rate as usize / 10) * self.output_channels as usize;
        let mut mixed_samples = vec![0i16; target_samples];
        let mut frames_mixed = 0;
        
        for (source_name, gain, buffer) in &self.input_buffers {
            let reader_id = format!("mixer:{}:{}", self.name, source_name);
            
            if let Some(frame) = buffer.pop_for_reader(&reader_id) {
                frames_mixed += 1;
                
                // Einfaches Mixing (TODO: Resampling implementieren)
                let samples_to_mix = frame.samples.len().min(mixed_samples.len());
                for i in 0..samples_to_mix {
                    let sample = frame.samples[i] as f32 * *gain;
                    mixed_samples[i] = (mixed_samples[i] as f32 + sample)
                        .clamp(-32768.0, 32767.0) as i16;
                }
            }
        }
        
        if frames_mixed > 0 {
            // Master gain anwenden
            if self.master_gain != 1.0 {
                for sample in mixed_samples.iter_mut() {
                    *sample = (*sample as f32 * self.master_gain)
                        .clamp(-32768.0, 32767.0) as i16;
                }
            }
            
            Some(PcmFrame {
                utc_ns: crate::core::timestamp::utc_ns_now(),
                samples: mixed_samples,
                sample_rate: self.output_sample_rate,
                channels: self.output_channels,
            })
        } else {
            None
        }
    }
    
    pub fn is_connected(&self) -> bool {
        self.connected
    }
    
    pub fn get_active_inputs(&self) -> Vec<(String, String, f32)> {
        self.input_buffers.iter()
            .map(|(source, gain, _)| (source.clone(), format!("mixer:{}:{}", self.name, source), *gain))
            .collect()
    }
    
    pub fn get_config(&self) -> &MixerConfig {
        &self.config
    }
}

impl Processor for Mixer {
    fn name(&self) -> &str {
        &self.name
    }
    
    fn process(&mut self, _input_buffer: &AudioRingBuffer, output_buffer: &AudioRingBuffer) -> Result<()> {
        if !self.connected {
            // Versuche automatisch zu verbinden, falls Registry verfügbar
            if self.buffer_registry.is_some() && self.config.auto_connect.unwrap_or(true) {
                if let Err(e) = self.connect_from_registry() {
                    self.error(&format!("Failed to auto-connect: {}", e));
                    return Ok(()); // Nicht fatal, einfach überspringen
                }
            }
            
            if !self.connected {
                self.warn("Mixer not connected, skipping processing");
                return Ok(());
            }
        }
        
        if let Some(mixed_frame) = self.mix_frame() {
            output_buffer.push(mixed_frame);
            
            // Status logging alle 200 Frames
            static mut FRAME_COUNT: u64 = 0;
            unsafe {
                FRAME_COUNT += 1;
                if FRAME_COUNT % 200 == 0 {
                    let avg_buffer: f32 = self.input_buffers.iter()
                        .map(|(_, _, b)| b.len() as f32)
                        .sum::<f32>() / self.input_buffers.len() as f32;
                    
                    self.info(&format!(
                        "Processed {} frames, avg input buffer: {:.1}, active inputs: {}",
                        FRAME_COUNT, avg_buffer, self.input_buffers.len()
                    ));
                }
            }
        }
        
        Ok(())
    }
    
    fn status(&self) -> ProcessorStatus {
        let buffer_levels: Vec<usize> = self.input_buffers.iter()
            .map(|(_, _, b)| b.len())
            .collect();
        
        let avg_buffer = if !buffer_levels.is_empty() {
            buffer_levels.iter().sum::<usize>() as f32 / buffer_levels.len() as f32
        } else { 0.0 };
        
        ProcessorStatus {
            running: self.connected && !self.input_buffers.is_empty(),
            processing_rate_hz: 10.0, // 100ms frames = 10Hz
            latency_ms: avg_buffer * 100.0, // ~100ms pro Frame
            errors: 0,
        }
    }
    
fn update_config(&mut self, config: serde_json::Value) -> Result<()> {
    match serde_json::from_value::<MixerConfig>(config) {
        Ok(mixer_config) => {
            self.update_config(&mixer_config)  // Referenz übergeben
        }
        Err(e) => {
            self.error(&format!("Failed to parse mixer config: {}", e));
            bail!("Invalid mixer config: {}", e)
        }
    }
}
    
    // Typ-Casting Methoden
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl ComponentLogger for Mixer {
    fn log_context(&self) -> LogContext {
        LogContext::new("Mixer", &self.name)
    }
}

// Unit Tests
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ringbuffer::AudioRingBuffer;

    #[test]
    fn test_mixer_creation() {
        let mixer = Mixer::new("test_mixer");
        assert_eq!(mixer.name(), "test_mixer");
        assert!(!mixer.is_connected());
    }

    #[test]
    fn test_mixer_from_config() {
        let config = MixerConfig {
            inputs: vec![
                MixerInputConfig {
                    name: "mic".to_string(),
                    gain: 0.8,
                    source: "mic_producer".to_string(),
                    enabled: Some(true),
                },
            ],
            output_sample_rate: Some(44100),
            output_channels: Some(1),
            master_gain: Some(0.9),
            auto_connect: Some(true),
        };
        
        let mixer = Mixer::from_config("test_mixer", config);
        
        assert_eq!(mixer.name(), "test_mixer");
        assert_eq!(mixer.output_sample_rate, 44100);
        assert_eq!(mixer.output_channels, 1);
        assert_eq!(mixer.master_gain, 0.9);
        assert!(!mixer.is_connected());
    }
}
