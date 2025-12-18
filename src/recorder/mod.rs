// src/recorder/mod.rs

use std::time::Duration;
use crate::ring::audio_ring::AudioSlot;

pub struct RecorderConfig {
    pub idle_sleep: Duration,
    pub retention_interval: Duration,
}

pub trait AudioSink: Send {
    fn on_chunk(&mut self, slot: &AudioSlot) -> anyhow::Result<()>;
    fn on_hour_change(&mut self, hour: u64) -> anyhow::Result<()>;
}

pub trait RetentionPolicy: Send {
    fn run(&mut self, now_utc_hour: u64) -> anyhow::Result<()>;
}

pub mod recorder;
pub mod sink_wav;
pub mod sink_mp3;
pub mod retention_fs;

pub use recorder::run_recorder;
pub use sink_wav::WavSink;
pub use sink_mp3::Mp3Sink;
pub use retention_fs::FsRetention;
