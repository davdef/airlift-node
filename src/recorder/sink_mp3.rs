// src/recorder/sink_mp3.rs
use crate::recorder::AudioSink;
use crate::ring::audio_ring::AudioSlot;

use std::path::PathBuf;
use std::process::Command;
use std::fs;

pub struct Mp3Sink {
    wav_dir: PathBuf,
    mp3_dir: PathBuf,
    bitrate: u32,
    last_hour: Option<u64>,
}

impl Mp3Sink {
    pub fn new(
        wav_dir: PathBuf,
        mp3_dir: PathBuf,
        bitrate: u32,
    ) -> anyhow::Result<Self> {
        fs::create_dir_all(&mp3_dir)?;
        Ok(Self {
            wav_dir,
            mp3_dir,
            bitrate,
            last_hour: None,
        })
    }

    fn transcode_hour(&self, hour: u64) -> anyhow::Result<()> {
        let wav = self.wav_dir.join(format!("{}.wav", hour));
        let mp3 = self.mp3_dir.join(format!("{}.mp3", hour));

        if !wav.exists() || mp3.exists() {
            return Ok(()); // nichts zu tun
        }

        eprintln!("[mp3_sink] ffmpeg {} → {}", wav.display(), mp3.display());

        let status = Command::new("ffmpeg")
            .args([
                "-y",
                "-loglevel", "quiet",
                "-i", wav.to_str().unwrap(),
                "-codec:a", "libmp3lame",
                "-b:a", &format!("{}k", self.bitrate),
                mp3.to_str().unwrap(),
            ])
            .status()?;

        if !status.success() {
            anyhow::bail!("ffmpeg failed for hour {}", hour);
        }

        Ok(())
    }
}

impl AudioSink for Mp3Sink {
    fn on_hour_change(&mut self, hour: u64) -> anyhow::Result<()> {
        // vorherige Stunde transkodieren
        if let Some(prev) = self.last_hour {
            self.transcode_hour(prev)?;
        }
        self.last_hour = Some(hour);
        Ok(())
    }

    fn on_chunk(&mut self, _slot: &AudioSlot) -> anyhow::Result<()> {
        // bewusst leer
        Ok(())
    }
}
