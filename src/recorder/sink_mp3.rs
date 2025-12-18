// src/recorder/sink_mp3.rs - MIT Send + Sync
use crate::recorder::AudioSink;
use crate::ring::audio_ring::AudioSlot;

use std::fs::{create_dir_all};
use std::path::{PathBuf};
use std::process::{Command, Stdio};
use std::io::Write;

pub struct Mp3Sink {
    wav_dir: PathBuf,
    mp3_dir: PathBuf,
    bitrate: u32,
    current_hour: Option<u64>,
    ffmpeg: Option<std::process::Child>,
}

// Explizit Send + Sync
unsafe impl Send for Mp3Sink {}
unsafe impl Sync for Mp3Sink {}

impl Mp3Sink {
    pub fn new(wav_dir: PathBuf, mp3_dir: PathBuf, bitrate: u32) -> anyhow::Result<Self> {
        create_dir_all(&mp3_dir)?;
        Ok(Self {
            wav_dir,
            mp3_dir,
            bitrate,
            current_hour: None,
            ffmpeg: None,
        })
    }

    fn start_ffmpeg(&mut self, hour: u64) -> anyhow::Result<()> {
        if let Some(mut child) = self.ffmpeg.take() {
            let _ = child.kill();
            let _ = child.wait();
        }

        let mut mp3_path = self.mp3_dir.clone();
        mp3_path.push(format!("{}.mp3", hour));

        let child = Command::new("ffmpeg")
            .args(&[
                "-loglevel", "error",
                "-f", "s16le",
                "-ar", "48000",
                "-ac", "2",
                "-i", "pipe:0",
                "-acodec", "libmp3lame",
                "-b:a", &format!("{}k", self.bitrate),
                "-y",
                mp3_path.to_str().unwrap(),
            ])
            .stdin(Stdio::piped())
            .spawn()?;

        self.ffmpeg = Some(child);
        self.current_hour = Some(hour);
        Ok(())
    }
}

impl AudioSink for Mp3Sink {
    fn on_hour_change(&mut self, hour: u64) -> anyhow::Result<()> {
        if self.current_hour != Some(hour) {
            self.start_ffmpeg(hour)?;
        }
        Ok(())
    }

    fn on_chunk(&mut self, slot: &AudioSlot) -> anyhow::Result<()> {
        let hour = slot.utc_ns / 1_000_000_000 / 3600;
        if self.current_hour != Some(hour) {
            self.on_hour_change(hour)?;
        }

        if let Some(ref mut child) = self.ffmpeg {
            if let Some(ref mut stdin) = child.stdin {
                let bytes = bytemuck::cast_slice::<i16, u8>(&slot.pcm);
                stdin.write_all(bytes)?;
                stdin.flush()?;
            }
        }

        Ok(())
    }
    
    fn maintain_continuity(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

impl Drop for Mp3Sink {
    fn drop(&mut self) {
        if let Some(mut child) = self.ffmpeg.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
