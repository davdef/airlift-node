// src/recorder/sink_wav.rs
use crate::recorder::AudioSink;
use crate::ring::audio_ring::AudioSlot;

use hound::{SampleFormat, WavSpec, WavWriter};
use std::fs::{create_dir_all};
use std::path::{PathBuf};

const RATE: u32 = 48_000;
const CHANNELS: u16 = 2;
const BITS: u16 = 16;

pub struct WavSink {
    base_dir: PathBuf,
    current_hour: Option<u64>,
    writer: Option<WavWriter<std::io::BufWriter<std::fs::File>>>,
}

impl WavSink {
    pub fn new(base_dir: PathBuf) -> anyhow::Result<Self> {
        create_dir_all(&base_dir)?;
        Ok(Self {
            base_dir,
            current_hour: None,
            writer: None,
        })
    }

    fn open_writer(&mut self, hour: u64) -> anyhow::Result<()> {
        // alten Writer schließen
        if let Some(w) = self.writer.take() {
            w.finalize()?;
        }

        let mut path = self.base_dir.clone();
        path.push(format!("{}.wav", hour)); // bewusst maschinenfreundlich

        let spec = WavSpec {
            channels: CHANNELS,
            sample_rate: RATE,
            bits_per_sample: BITS,
            sample_format: SampleFormat::Int,
        };

        let w = WavWriter::create(&path, spec)?;
        self.writer = Some(w);
        self.current_hour = Some(hour);

        eprintln!("[wav_sink] new file {:?}", path);
        Ok(())
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
        if let Some(w) = self.writer.as_mut() {
            for s in slot.pcm.iter() {
                w.write_sample(*s)?;
            }
        }
        Ok(())
    }
}
