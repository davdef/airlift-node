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
    input_buffers: Vec<MixerInputBuffer>,
    output_sample_rate: u32,
    output_channels: u8,
    master_gain: f32,
    buffer_registry: Option<Arc<BufferRegistry>>,
    connected: bool,
}

struct MixerInputBuffer {
    source_name: String,
    reader_id: String,
    gain: f32,
    buffer: Arc<AudioRingBuffer>,
}

const MAX_BATCH_FRAMES: usize = 8;

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
                    self.input_buffers.push(MixerInputBuffer {
                        source_name: input_config.source.clone(),
                        reader_id: format!("mixer:{}:{}", self.name, input_config.source),
                        gain: input_config.gain,
                        buffer,
                    });
                    
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
        self.input_buffers.retain(|input| input.source_name != source_name);

        self.input_buffers.push(MixerInputBuffer {
            source_name: source_name.to_string(),
            reader_id: format!("mixer:{}:{}", self.name, source_name),
            gain,
            buffer,
        });
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
    fn mix_batch(&self, batch_size: usize) -> Vec<PcmFrame> {
        if !self.connected || self.input_buffers.is_empty() {
            return Vec::new();
        }

        let target_samples = (self.output_sample_rate as usize / 10) * self.output_channels as usize;
        let mut mixed_frames = Vec::with_capacity(batch_size);

        for _ in 0..batch_size {
            let mut mixed_samples = vec![0i16; target_samples];
            let mut frames_mixed = 0;

            for input in &self.input_buffers {
                if let Some(frame) = input.buffer.pop_for_reader(&input.reader_id) {
                    frames_mixed += 1;
                    self.mix_samples(&mut mixed_samples, &frame.samples, input.gain);
                }
            }

            if frames_mixed == 0 {
                break;
            }

            self.apply_master_gain(&mut mixed_samples);

            mixed_frames.push(PcmFrame {
                utc_ns: crate::core::timestamp::utc_ns_now(),
                samples: mixed_samples,
                sample_rate: self.output_sample_rate,
                channels: self.output_channels,
            });
        }

        mixed_frames
    }

    fn mix_samples(&self, mixed_samples: &mut [i16], input_samples: &[i16], gain: f32) {
        let samples_to_mix = input_samples.len().min(mixed_samples.len());
        for i in 0..samples_to_mix {
            let sample = input_samples[i] as f32 * gain;
            mixed_samples[i] = (mixed_samples[i] as f32 + sample)
                .clamp(-32768.0, 32767.0) as i16;
        }
    }

    fn apply_master_gain(&self, samples: &mut [i16]) {
        if self.master_gain != 1.0 {
            for sample in samples.iter_mut() {
                *sample = (*sample as f32 * self.master_gain)
                    .clamp(-32768.0, 32767.0) as i16;
            }
        }
    }
    
    pub fn is_connected(&self) -> bool {
        self.connected
    }
    
    pub fn get_active_inputs(&self) -> Vec<(String, String, f32)> {
        self.input_buffers.iter()
            .map(|input| (input.source_name.clone(), input.reader_id.clone(), input.gain))
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
        
        let mut max_available = 0;
        for input in &self.input_buffers {
            max_available = max_available.max(input.buffer.available_for_reader(&input.reader_id));
        }

        if max_available == 0 {
            return Ok(());
        }

        let batch_size = max_available.min(MAX_BATCH_FRAMES);
        let mixed_frames = self.mix_batch(batch_size);
        let mixed_count = mixed_frames.len();
        if mixed_count == 0 {
            return Ok(());
        }

        for frame in mixed_frames {
            output_buffer.push(frame);
        }

        // Status logging alle 200 Frames
        static mut FRAME_COUNT: u64 = 0;
        unsafe {
            FRAME_COUNT += mixed_count as u64;
            if FRAME_COUNT % 200 == 0 {
                let avg_buffer: f32 = self.input_buffers.iter()
                    .map(|input| input.buffer.len() as f32)
                    .sum::<f32>() / self.input_buffers.len() as f32;

                self.info(&format!(
                    "Processed {} frames, avg input buffer: {:.1}, active inputs: {}",
                    FRAME_COUNT, avg_buffer, self.input_buffers.len()
                ));
            }
        }
        
        Ok(())
    }
    
    fn status(&self) -> ProcessorStatus {
        let buffer_levels: Vec<usize> = self.input_buffers.iter()
            .map(|input| input.buffer.len())
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
