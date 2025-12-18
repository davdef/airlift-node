// src/recorder/config.rs
use std::time::Duration;

pub struct RecorderConfig {
    pub idle_sleep: Duration,
    pub retention_interval: Duration,
}
