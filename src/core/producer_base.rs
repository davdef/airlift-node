// src/core/producer_base.rs - Generische Producer-Logik

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use anyhow::Result;

pub struct ProducerBase {
    name: String,
    running: Arc<AtomicBool>,
    samples_processed: Arc<AtomicU64>,
    buffer: Option<Arc<dyn PcmWriter>>,
    
    // PCM-Konfiguration
    sample_rate: u32,
    channels: u8,
    target_frames: usize,
    
    // Signal-Analyse
    peak_monitor: PeakMonitor,
    format_detector: FormatDetector,
}

impl ProducerBase {
    pub fn new(name: &str, sample_rate: u32, channels: u8) -> Self {
        Self {
            name: name.to_string(),
            running: Arc::new(AtomicBool::new(false)),
            samples_processed: Arc::new(AtomicU64::new(0)),
            buffer: None,
            sample_rate,
            channels,
            target_frames: (sample_rate as usize) / 10,  // 100ms
        }
    }
    
    pub fn attach_buffer(&mut self, buffer: Arc<dyn PcmWriter>) {
        self.buffer = Some(buffer);
    }
    
    pub fn write_pcm_chunk(&self, pcm_samples: Vec<i16>) -> Result<()> {
        if let Some(buffer) = &self.buffer {
            let utc_ns = utc_ns_now() - 100_000_000;  // Latenz-Kompensation
            buffer.write(utc_ns, pcm_samples)?;
            self.samples_processed.fetch_add(
                pcm_samples.len() as u64, 
                Ordering::Relaxed
            );
        }
        Ok(())
    }
    
    // FIFO-Handling (wie in deinem Code)
    pub fn process_fifo(&self, fifo: &mut Vec<i16>, new_samples: &[i16]) -> Result<()> {
        fifo.extend_from_slice(new_samples);
        
        while fifo.len() >= self.target_frames * self.channels as usize {
            let chunk: Vec<i16> = fifo.drain(..self.target_frames * self.channels as usize).collect();
            self.write_pcm_chunk(chunk)?;
        }
        
        Ok(())
    }
    
    // UTC-Hilfsfunktion (wie in deinem Code)
    pub fn utc_ns_now() -> u64 {
        let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        d.as_secs() * 1_000_000_000 + d.subsec_nanos() as u64
    }

    pub fn analyze_signal(&self, samples: &[i16]) -> SignalAnalysis {
        self.peak_monitor.update(samples);
        self.format_detector.feed(samples);
        
        SignalAnalysis {
            peaks: self.peak_monitor.current_peaks(),
            rms: self.peak_monitor.current_rms(),
            format_hint: self.format_detector.best_guess(),
            clipping: self.peak_monitor.clipping_detected(),
        }
    }
    
    /// Scannt verfügbare Geräte (implementierungsspezifisch)
    pub fn scan_available_devices(&self) -> Result<Vec<AudioDeviceInfo>> {
        // Default-Implementierung (kann überschrieben werden)
        Ok(Vec::new())
    }
}

#[derive(Debug, Clone)]
pub struct SignalAnalysis {
    pub peaks: Vec<f32>,      // Maximalpeaks pro Kanal [-1.0..1.0]
    pub rms: Vec<f32>,        // RMS pro Kanal
    pub format_hint: Option<AudioFormat>,
    pub clipping: bool,
}
