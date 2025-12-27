use std::sync::{Arc, Mutex};
use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub struct PcmFrame {
    pub utc_ns: u64,
    pub samples: Vec<i16>,
    pub sample_rate: u32,
    pub channels: u8,
}

pub struct AudioRingBuffer {
    buffer: Arc<Mutex<VecDeque<PcmFrame>>>,
    capacity: usize,
    dropped_frames: Arc<Mutex<u64>>,
}

impl AudioRingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: Arc::new(Mutex::new(VecDeque::with_capacity(capacity))),
            capacity,
            dropped_frames: Arc::new(Mutex::new(0)),
        }
    }
    
    pub fn push(&self, frame: PcmFrame) -> u64 {
        let mut buffer = self.buffer.lock().unwrap();
        
        if buffer.len() >= self.capacity {
            buffer.pop_front();
            let mut dropped = self.dropped_frames.lock().unwrap();
            *dropped += 1;
        }
        
        buffer.push_back(frame);
        buffer.len() as u64
    }
    
    pub fn pop(&self) -> Option<PcmFrame> {
        let mut buffer = self.buffer.lock().unwrap();
        buffer.pop_front()
    }
    
    pub fn clear(&self) {
        let mut buffer = self.buffer.lock().unwrap();
        buffer.clear();
    }
    
    pub fn len(&self) -> usize {
        let buffer = self.buffer.lock().unwrap();
        buffer.len()
    }
    
    pub fn is_empty(&self) -> bool {
        let buffer = self.buffer.lock().unwrap();
        buffer.is_empty()
    }
    
    pub fn stats(&self) -> RingBufferStats {
        let buffer = self.buffer.lock().unwrap();
        let dropped = *self.dropped_frames.lock().unwrap();
        
        RingBufferStats {
            capacity: self.capacity,
            current_frames: buffer.len(),
            dropped_frames: dropped,
            latest_timestamp: buffer.back().map(|f| f.utc_ns),
            oldest_timestamp: buffer.front().map(|f| f.utc_ns),
        }
    }
    
    pub fn iter(&self) -> RingBufferIter {
        RingBufferIter {
            buffer: self.buffer.clone(),
            index: 0,
        }
    }
}

pub struct RingBufferIter {
    buffer: Arc<Mutex<VecDeque<PcmFrame>>>,
    index: usize,
}

impl Iterator for RingBufferIter {
    type Item = PcmFrame;
    
    fn next(&mut self) -> Option<Self::Item> {
        let buffer = self.buffer.lock().unwrap();
        if self.index < buffer.len() {
            // Clone the frame at index
            let frame = buffer.get(self.index).cloned();
            self.index += 1;
            frame
        } else {
            None
        }
    }
}

#[derive(Debug, Clone)]
pub struct RingBufferStats {
    pub capacity: usize,
    pub current_frames: usize,
    pub dropped_frames: u64,
    pub latest_timestamp: Option<u64>,
    pub oldest_timestamp: Option<u64>,
}
