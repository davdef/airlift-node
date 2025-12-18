// src/recorder/mod.rs

mod sink_wav;
mod sink_mp3;
mod recorder;
mod retention_fs;

pub use sink_wav::WavSink;
pub use sink_mp3::Mp3Sink;
pub use recorder::{run_recorder};

use crate::ring::audio_ring::AudioSlot;
use std::time::Duration;
pub use retention_fs::FsRetention;

pub trait AudioSink: Send + Sync {
    fn on_hour_change(&mut self, hour: u64) -> anyhow::Result<()>;
    fn on_chunk(&mut self, slot: &AudioSlot) -> anyhow::Result<()>;
    fn maintain_continuity(&mut self) -> anyhow::Result<()>;
}

pub trait RetentionPolicy: Send + Sync {
    fn run(&mut self, current_hour: u64) -> anyhow::Result<()>;
}

pub struct RecorderConfig {
    pub idle_sleep: Duration,
    pub retention_interval: Duration,
    pub continuity_interval: Duration,
}
