use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct PcmFrame {
    pub utc_ns: u64,
    pub samples: Vec<i16>,
    pub sample_rate: u32,
    pub channels: u8,
}

struct RingBufferInner {
    buffer: VecDeque<PcmFrame>,
    start_index: u64,
    next_index: u64,
    read_positions: HashMap<String, u64>,
}

pub struct AudioRingBuffer {
    inner: Arc<Mutex<RingBufferInner>>,
    capacity: usize,
    dropped_frames: Arc<Mutex<u64>>,
}

impl AudioRingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(RingBufferInner {
                buffer: VecDeque::with_capacity(capacity),
                start_index: 0,
                next_index: 0,
                read_positions: HashMap::new(),
            })),
            capacity,
            dropped_frames: Arc::new(Mutex::new(0)),
        }
    }
    
    pub fn push(&self, frame: PcmFrame) -> u64 {
        let mut inner = self.inner.lock().unwrap();
        
        if inner.buffer.len() >= self.capacity {
            inner.buffer.pop_front();
            inner.start_index += 1;
            let mut dropped = self.dropped_frames.lock().unwrap();
            *dropped += 1;
        }
        
        inner.buffer.push_back(frame);
        inner.next_index += 1;
        
        for position in inner.read_positions.values_mut() {
            if *position < inner.start_index {
                *position = inner.start_index;
            }
        }
        
        inner.buffer.len() as u64
    }
    
    pub fn pop(&self) -> Option<PcmFrame> {
        self.pop_for_reader("default")
    }
    
    pub fn pop_for_reader(&self, reader_id: &str) -> Option<PcmFrame> {
        let mut inner = self.inner.lock().unwrap();
        let position = inner.read_positions
            .entry(reader_id.to_string())
            .or_insert(inner.start_index);
        
        if *position < inner.start_index {
            *position = inner.start_index;
        }
        
        if *position >= inner.next_index {
            return None;
        }
        
        let offset = (*position - inner.start_index) as usize;
        let frame = inner.buffer.get(offset).cloned();
        if frame.is_some() {
            *position += 1;
        }
        
        let min_position = inner.read_positions.values().copied().min().unwrap_or(inner.next_index);
        while inner.start_index < min_position {
            if inner.buffer.pop_front().is_some() {
                inner.start_index += 1;
            } else {
                break;
            }
        }
        
        frame
    }
    
    pub fn clear(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.buffer.clear();
        inner.start_index = inner.next_index;
        for position in inner.read_positions.values_mut() {
            *position = inner.start_index;
        }
    }
    
    pub fn len(&self) -> usize {
        let inner = self.inner.lock().unwrap();
        inner.buffer.len()
    }
    
    pub fn is_empty(&self) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.buffer.is_empty()
    }
    
    pub fn stats(&self) -> RingBufferStats {
        let inner = self.inner.lock().unwrap();
        let dropped = *self.dropped_frames.lock().unwrap();
        
        RingBufferStats {
            capacity: self.capacity,
            current_frames: inner.buffer.len(),
            dropped_frames: dropped,
            latest_timestamp: inner.buffer.back().map(|f| f.utc_ns),
            oldest_timestamp: inner.buffer.front().map(|f| f.utc_ns),
        }
    }
    
    pub fn iter(&self) -> RingBufferIter {
        let inner = self.inner.lock().unwrap();
        RingBufferIter { buffer: inner.buffer.iter().cloned().collect(), index: 0 }
    }
}

pub struct RingBufferIter {
    buffer: Vec<PcmFrame>,
    index: usize,
}

impl Iterator for RingBufferIter {
    type Item = PcmFrame;
    
    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.buffer.len() {
            let frame = self.buffer.get(self.index).cloned();
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
