// src/recorder/mod.rs

mod recorder;
mod retention_fs;

use std::time::Duration;

use crate::codecs::EncodedFrame;

pub use recorder::run_recorder;
pub use retention_fs::FsRetention;

pub enum EncodedRead {
    Frame { frame: EncodedFrame, utc_ns: u64 },
    Gap { missed: u64 },
    Empty,
}

pub trait EncodedFrameSource: Send {
    fn poll(&mut self) -> anyhow::Result<EncodedRead>;
}

pub trait RetentionPolicy: Send + Sync {
    fn run(&mut self, current_hour: u64) -> anyhow::Result<()>;
}

pub struct RecorderConfig {
    pub idle_sleep: Duration,
    pub retention_interval: Duration,
}
