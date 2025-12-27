# Mixer verbessern für Flow-Integration
cat > src/processors/mixer.rs << 'EOF'
use crate::core::processor::{Processor, ProcessorStatus};
use crate::core::ringbuffer::{AudioRingBuffer, PcmFrame};
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;

pub struct Mixer {
    name: String,
    input_gains: HashMap<String, f32>,
    input_buffers: HashMap<String, Arc<AudioRingBuffer>>,
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
    
    pub fn add_input(&mut self, input_name: &str, buffer: Arc<AudioRingBuffer>, gain: f32) {
        self.input_buffers.insert(input_name.to_string(), buffer);
        self.input_gains.insert(input_name.to_string(), gain.clamp(0.0, 1.0));
        log::info!("Mixer '{}': Added input '{}' with gain {}", self.name, input_name, gain);
    }
    
    pub fn set_output_format(&mut self, sample_rate: u32, channels: u8) {
        self.output_sample_rate = sample_rate;
        self.output_channels = channels;
    }
    
    fn mix_available_frames(&self) -> Option<PcmFrame> {
        if self.input_buffers.is_empty() {
            return None;
        }
        
        let target_samples = (self.output_sample_rate as usize / 10) * self.output_channels as usize;
        let mut mixed_samples = vec![0i16; target_samples];
        
        // Für jeden Input: nehme ein Frame wenn verfügbar
        for (input_name, buffer) in &self.input_buffers {
            if let Some(frame) = buffer.pop() {
                let gain = self.input_gains.get(input_name).copied().unwrap_or(1.0);
                
                // Einfaches Mixing (ohne Resampling)
                let samples_to_mix = frame.samples.len().min(mixed_samples.len());
                for i in 0..samples_to_mix {
                    let mixed = mixed_samples[i] as f32 + (frame.samples[i] as f32 * gain);
                    mixed_samples[i] = mixed.clamp(-32768.0, 32767.0) as i16;
                }
                
                log::debug!("Mixer '{}': Mixed frame from '{}' (gain: {})", 
                    self.name, input_name, gain);
            }
        }
        
        // Nur wenn wir etwas gemischt haben
        if mixed_samples.iter().any(|&s| s != 0) {
            Some(PcmFrame {
                utc_ns: crate::core::utc_ns_now(),
                samples: mixed_samples,
                sample_rate: self.output_sample_rate,
                channels: self.output_channels,
            })
        } else {
            None
        }
    }
}

impl Processor for Mixer {
    fn name(&self) -> &str {
        &self.name
    }
    
    fn process(&mut self, input_buffer: &AudioRingBuffer, output_buffer: &AudioRingBuffer) -> Result<()> {
        // Mixer ignoriert den single input_buffer Parameter
        // und nutzt seine eigenen input_buffers
        
        if let Some(mixed_frame) = self.mix_available_frames() {
            output_buffer.push(mixed_frame);
            log::debug!("Mixer '{}': Pushed mixed frame to output", self.name);
        }
        
        Ok(())
    }
    
    fn status(&self) -> ProcessorStatus {
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
                    if let Some(g) = self.input_gains.get_mut(input_name) {
                        *g = gain as f32;
                    } else {
                        self.input_gains.insert(input_name.clone(), gain as f32);
                    }
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

# Flow muss Mixer mit Input-Buffern verbinden
echo "Mixer angepasst. Jetzt muss der Flow den Mixer mit den Input-Buffern verbinden."
echo "Wir brauchen eine Möglichkeit, den Mixer zu konfigurieren, welche Inputs er nutzen soll."
echo ""
echo "Optionen:"
echo "1. Mixer über Flow-Config Input-Namen zuweisen"
echo "2. Mixer automatisch alle Flow-Input-Buffers nutzen"
echo "3. Mixer über Processor-Config Input-Mapping"

echo "Was bevorzugst du?"
