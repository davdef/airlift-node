// src/recorder/sink_wav.rs - MIT Send + Sync
use crate::recorder::AudioSink;
use crate::ring::audio_ring::AudioSlot;

use hound::{SampleFormat, WavSpec, WavWriter};
use std::fs::{create_dir_all};
use std::path::{PathBuf};
use std::time::Instant;

const RATE: u32 = 48_000;
const CHANNELS: u16 = 2;
const BITS: u16 = 16;
const SAMPLES_PER_SECOND: u64 = (RATE * CHANNELS as u32) as u64;
const SAMPLES_PER_HOUR: u64 = SAMPLES_PER_SECOND * 3600;

pub struct WavSink {
    base_dir: PathBuf,
    writer: Option<WavWriter<std::io::BufWriter<std::fs::File>>>,
    current_hour: Option<u64>,
    position_in_hour: u64,
    last_audio_time: Instant,
    hour_start_time: Option<Instant>,
}

// Explizit Send + Sync implementieren (hound::WavWriter ist Send + Sync)
unsafe impl Send for WavSink {}
unsafe impl Sync for WavSink {}

impl WavSink {
    pub fn new(base_dir: PathBuf) -> anyhow::Result<Self> {
        create_dir_all(&base_dir)?;
        Ok(Self {
            base_dir,
            writer: None,
            current_hour: None,
            position_in_hour: 0,
            last_audio_time: Instant::now(),
            hour_start_time: None,
        })
    }

    fn open_writer(&mut self, hour: u64) -> anyhow::Result<()> {
        if let Some(w) = self.writer.take() {
            self.fill_to_hour_end()?;
            w.finalize()?;
            eprintln!("[wav_sink] finalized hour {}", self.current_hour.unwrap_or(0));
        }

        let mut path = self.base_dir.clone();
        path.push(format!("{}.wav", hour));

        let spec = WavSpec {
            channels: CHANNELS,
            sample_rate: RATE,
            bits_per_sample: BITS,
            sample_format: SampleFormat::Int,
        };

        let writer = WavWriter::create(&path, spec)?;
        
        self.writer = Some(writer);
        self.current_hour = Some(hour);
        self.position_in_hour = 0;
        self.hour_start_time = Some(Instant::now());
        
        eprintln!("[wav_sink] new file {:?} (will contain 3600s)", path);
        Ok(())
    }
    
    fn write_silence(&mut self, samples: u64) -> anyhow::Result<()> {
        if samples == 0 || self.writer.is_none() {
            return Ok(());
        }
        
        if samples > 1000 {
            eprintln!("[wav_sink] writing {} silent samples", samples);
        }
        
        const SILENCE: i16 = 0;
        let block_size = 1024 * CHANNELS as u64;
        
        let mut remaining = samples;
        while remaining > 0 {
            let to_write = remaining.min(block_size);
            for _ in 0..to_write {
                self.writer.as_mut().unwrap().write_sample(SILENCE)?;
            }
            remaining -= to_write;
        }
        
        self.position_in_hour += samples;
        Ok(())
    }
    
    fn fill_to_hour_end(&mut self) -> anyhow::Result<()> {
        if self.position_in_hour < SAMPLES_PER_HOUR {
            let missing = SAMPLES_PER_HOUR - self.position_in_hour;
            eprintln!("[wav_sink] filling {} samples to complete hour", missing);
            self.write_silence(missing)?;
        }
        Ok(())
    }
    
    fn calculate_missing_samples(&self) -> u64 {
        let elapsed = self.last_audio_time.elapsed();
        let expected_samples = (elapsed.as_secs_f64() * SAMPLES_PER_SECOND as f64) as u64;
        
        if let Some(hour_start) = self.hour_start_time {
            let total_elapsed = hour_start.elapsed();
            let total_expected = (total_elapsed.as_secs_f64() * SAMPLES_PER_SECOND as f64) as u64;
            expected_samples.min(total_expected.saturating_sub(self.position_in_hour))
        } else {
            expected_samples
        }
    }

    fn align_missing_samples(&self, missing_samples: u64) -> u64 {
        let remainder = missing_samples % CHANNELS as u64;
        if remainder != 0 && cfg!(debug_assertions) {
            eprintln!(
                "[wav_sink] missing samples not aligned to channels: {} (remainder {})",
                missing_samples, remainder
            );
        }
        missing_samples - remainder
    }
}

impl AudioSink for WavSink {
    fn on_hour_change(&mut self, hour: u64) -> anyhow::Result<()> {
        if self.current_hour != Some(hour) {
            self.open_writer(hour)?;
        }
        Ok(())
    }

    fn on_chunk(&mut self, slot: &AudioSlot) -> anyhow::Result<()> {
        let hour = slot.utc_ns / 1_000_000_000 / 3600;
        if self.current_hour != Some(hour) {
            self.on_hour_change(hour)?;
        }
        
        let missing_samples = self.calculate_missing_samples();
        let aligned_missing_samples = self.align_missing_samples(missing_samples);
        if aligned_missing_samples > 0 {
            self.write_silence(aligned_missing_samples)?;
        }
        
        if let Some(w) = self.writer.as_mut() {
            let samples_in_chunk = slot.pcm.len() as u64;
            
            for s in slot.pcm.iter() {
                w.write_sample(*s)?;
            }
            
            self.position_in_hour += samples_in_chunk;
            self.last_audio_time = Instant::now();
        }
        
        Ok(())
    }
    
    fn maintain_continuity(&mut self) -> anyhow::Result<()> {
        if self.writer.is_none() {
            return Ok(());
        }
        
        let missing_samples = self.calculate_missing_samples();
        let aligned_missing_samples = self.align_missing_samples(missing_samples);
        let missing_ms = (aligned_missing_samples * 1000) / SAMPLES_PER_SECOND;
        
        if missing_ms > 100 {
            self.write_silence(aligned_missing_samples)?;
            self.last_audio_time = Instant::now();
            
            if missing_ms > 1000 {
                eprintln!("[wav_sink] maintained continuity: added {}ms silence", missing_ms);
            }
        }
        
        Ok(())
    }
}

impl Drop for WavSink {
    fn drop(&mut self) {
        if let Some(w) = self.writer.take() {
            let _ = self.fill_to_hour_end();
            let _ = w.finalize();
            eprintln!("[wav_sink] dropped and finalized");
        }
    }
}
