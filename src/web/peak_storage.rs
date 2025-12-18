use std::collections::VecDeque;
use std::sync::Mutex;

use serde::Serialize;

use crate::io::peak_analyzer::PeakEvent;

pub struct PeakStorage {
    buffer: Mutex<VecDeque<PeakEvent>>,
    capacity: usize,
}

impl PeakStorage {
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: Mutex::new(VecDeque::with_capacity(capacity)),
            capacity,
        }
    }

    pub fn add_peak(&self, peak: PeakEvent) {
        let mut guard = self.buffer.lock().expect("peak buffer poisoned");
        if guard.len() >= self.capacity {
            guard.pop_front();
        }
        guard.push_back(peak);
    }

    pub fn get_latest(&self) -> Option<PeakEvent> {
        let guard = self.buffer.lock().expect("peak buffer poisoned");
        guard.back().cloned()
    }

    pub fn snapshot(&self, limit: usize) -> Vec<PeakEvent> {
        let guard = self.buffer.lock().expect("peak buffer poisoned");
        let len = guard.len();
        guard
            .iter()
            .skip(len.saturating_sub(limit))
            .cloned()
            .collect()
    }

    pub fn info(&self) -> PeakBufferInfo {
        let guard = self.buffer.lock().expect("peak buffer poisoned");
        PeakBufferInfo {
            capacity: self.capacity,
            len: guard.len(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PeakBufferInfo {
    pub capacity: usize,
    pub len: usize,
}
