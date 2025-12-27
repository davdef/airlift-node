# 1. Mixer Import in mixer.rs korrigieren
cat > src/processors/mixer.rs << 'EOF'
use crate::core::processor::{Processor, ProcessorStatus};
use crate::core::ringbuffer::{AudioRingBuffer, PcmFrame};
use anyhow::Result;
use std::collections::HashMap;

pub struct Mixer {
    name: String,
    input_gains: HashMap<String, f32>,
    input_buffers: HashMap<String, std::sync::Arc<AudioRingBuffer>>,
    output_sample_rate: u32,
    output_channels: u8,
}

impl Mixer {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            input_gains: HashMap::new(),
            input_buffers: HashMap::new(),
            output_sample_rate: 48000,
            output_channels: 2,
        }
    }
    
    pub fn add_input(&mut self, input_name: &str, buffer: std::sync::Arc<AudioRingBuffer>, gain: f32) {
        self.input_buffers.insert(input_name.to_string(), buffer);
        self.input_gains.insert(input_name.to_string(), gain.clamp(0.0, 1.0));
        log::info!("Mixer '{}': Added input '{}' with gain {}", self.name, input_name, gain);
    }
    
    pub fn remove_input(&mut self, input_name: &str) {
        self.input_buffers.remove(input_name);
        self.input_gains.remove(input_name);
    }
    
    pub fn set_gain(&mut self, input_name: &str, gain: f32) {
        if let Some(g) = self.input_gains.get_mut(input_name) {
            *g = gain.clamp(0.0, 1.0);
        }
    }
    
    pub fn set_output_format(&mut self, sample_rate: u32, channels: u8) {
        self.output_sample_rate = sample_rate;
        self.output_channels = channels;
    }
    
    fn mix_frames(&self, frames: Vec<Option<PcmFrame>>) -> Option<PcmFrame> {
        if frames.is_empty() {
            return None;
        }
        
        let target_samples = (self.output_sample_rate as usize / 10) * self.output_channels as usize;
        let mut mixed_samples = vec![0i16; target_samples];
        
        for (input_name, frame_opt) in frames.iter().enumerate() {
            if let Some(frame) = frame_opt {
                let gain = self.input_gains
                    .values()
                    .nth(input_name)
                    .copied()
                    .unwrap_or(1.0);
                
                let frame_samples = frame.samples.len().min(mixed_samples.len());
                for i in 0..frame_samples {
                    let mixed = mixed_samples[i] as f32 + (frame.samples[i] as f32 * gain);
                    mixed_samples[i] = mixed.clamp(-32768.0, 32767.0) as i16;
                }
            }
        }
        
        Some(PcmFrame {
            utc_ns: crate::core::utc_ns_now(),
            samples: mixed_samples,
            sample_rate: self.output_sample_rate,
            channels: self.output_channels,
        })
    }
}

impl Processor for Mixer {
    fn name(&self) -> &str {
        &self.name
    }
    
    fn process(&mut self, _input_buffer: &AudioRingBuffer, output_buffer: &AudioRingBuffer) -> Result<()> {
        let mut frames = Vec::new();
        
        for buffer in self.input_buffers.values() {
            frames.push(buffer.pop());
        }
        
        if let Some(mixed_frame) = self.mix_frames(frames) {
            output_buffer.push(mixed_frame);
        }
        
        Ok(())
    }
    
    fn status(&self) -> ProcessorStatus {
        let input_count = self.input_buffers.len();
        let buffer_levels: Vec<usize> = self.input_buffers.values().map(|b| b.len()).collect();
        let avg_buffer = if !buffer_levels.is_empty() {
            buffer_levels.iter().sum::<usize>() as f32 / buffer_levels.len() as f32
        } else { 0.0 };
        
        ProcessorStatus {
            running: true,
            processing_rate_hz: 10.0,
            latency_ms: avg_buffer as f32 * 100.0,
            errors: 0,
        }
    }
    
    fn update_config(&mut self, config: serde_json::Value) -> Result<()> {
        if let Some(gains) = config.get("gains").and_then(|v| v.as_object()) {
            for (input_name, gain_value) in gains {
                if let Some(gain) = gain_value.as_f64() {
                    self.set_gain(input_name, gain as f32);
                }
            }
        }
        
        if let Some(sample_rate) = config.get("sample_rate").and_then(|v| v.as_u64()) {
            self.output_sample_rate = sample_rate as u32;
        }
        
        if let Some(channels) = config.get("channels").and_then(|v| v.as_u64()) {
            self.output_channels = channels as u8;
        }
        
        Ok(())
    }
}
EOF

# 2. Main.rs Import korrigieren (nur eine Zeile ändern)
sed -i 's/use crate::processors::Mixer;//g' src/main.rs 2>/dev/null || true
sed -i 's/crate::crate::crate::processors/processors/g' src/main.rs 2>/dev/null || true
sed -i 's/crate::crate::processors/processors/g' src/main.rs 2>/dev/null || true
sed -i 's/crate::processors::mixer::Mixer/processors::mixer::Mixer/g' src/main.rs 2>/dev/null || true

# 3. Processors Modul-Datei sicherstellen
cat > src/processors/mod.rs << 'EOF'
pub mod mixer;
pub use mixer::Mixer;
EOF

# 4. Build
echo "Jetzt sollte es gehen:"
cargo build
