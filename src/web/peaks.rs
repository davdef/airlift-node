use std::collections::VecDeque;
use std::sync::{RwLock, RwLockReadGuard};

#[derive(Debug, Clone, serde::Serialize)]
pub struct PeakPoint {
    pub timestamp: u64,
    pub peak_l: f32,
    pub peak_r: f32,
    pub rms: Option<f32>,
    pub lufs: Option<f32>,
    pub silence: bool,
}

pub struct PeakStorage {
    buffer: RwLock<VecDeque<PeakPoint>>,
}

impl PeakStorage {
    pub fn new() -> Self {
        Self {
            buffer: RwLock::new(VecDeque::with_capacity(10_000)),
        }
    }

    pub fn add_peak(&self, peak: PeakPoint) {
        let mut buf = self.buffer.write().unwrap();
        buf.push_back(peak);
        if buf.len() > 10_000 {
            buf.pop_front();
        }
    }

    pub fn get_latest(&self) -> Option<PeakPoint> {
        self.buffer.read().unwrap().back().cloned()
    }

    pub fn get_buffer_read_lock(&self) -> Result<RwLockReadGuard<VecDeque<PeakPoint>>, ()> {
        self.buffer.read().map_err(|_| ())
    }
}
