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

    /// Push a frame into the ring.
    /// Returns the current number of frames in the buffer.
    pub fn push(&self, frame: PcmFrame) -> u64 {
        let mut inner = self.inner.lock().unwrap();

        // Platz schaffen, wenn voll
        if inner.buffer.len() >= self.capacity {
            inner.buffer.pop_front();
            inner.start_index += 1;
            let mut dropped = self.dropped_frames.lock().unwrap();
            *dropped += 1;
        }

        inner.buffer.push_back(frame);
        inner.next_index += 1;

        // Reader-Positionen auf den neuen gültigen Bereich clampen,
        // aber ohne inner während des Iterierens nochmal immutable zu leihen.
        let start_index = inner.start_index;
        for position in inner.read_positions.values_mut() {
            if *position < start_index {
                *position = start_index;
            }
        }

        inner.buffer.len() as u64
    }

    pub fn pop(&self) -> Option<PcmFrame> {
        self.pop_for_reader("default")
    }

    /// Leser-spezifisches Pop, mit Reader-ID (Multi-Reader).
pub fn pop_for_reader(&self, reader_id: &str) -> Option<PcmFrame> {
    let mut inner = self.inner.lock().unwrap();

    let start_index = inner.start_index;
    let next_index  = inner.next_index;

    // 1) Position aus HashMap holen → direkt kopieren → Borrow ist danach weg
    let pos = inner.read_positions
        .entry(reader_id.to_string())
        .or_insert(start_index);

    let mut position = *pos;

    if position < start_index {
        position = start_index;
    }
    if position >= next_index {
        return None;
    }

    // 2) Jetzt dürfen wir ohne Borrow-Probleme lesen
//    let offset = (position - start_index) as usize;
//    let frame_opt = inner.buffer.get(offset).cloned();

    // 3) Erst *jetzt* zurückschreiben → Borrow erst am Ende
//    if frame_opt.is_some() {
//        *inner.read_positions.get_mut(reader_id).unwrap() = position + 1;
//    }

let offset = (position - start_index) as usize;

// Borrow vermeiden: Frame erst holen, danach Position setzen
let frame_opt = {
    let frame = inner.buffer.get(offset).cloned();
    frame
};

if frame_opt.is_some() {
    *inner.read_positions.get_mut(reader_id).unwrap() = position + 1;
}

    // 4) Aufräumen alter Frames anhand minimaler Leser-Position
    let min_pos = inner.read_positions.values().copied().min().unwrap_or(next_index);
    while inner.start_index < min_pos {
        if inner.buffer.pop_front().is_some() {
            inner.start_index += 1;
        } else {
            break;
        }
    }

    frame_opt
}

    pub fn clear(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.buffer.clear();
        inner.start_index = inner.next_index;

        let start_index = inner.start_index;
        for position in inner.read_positions.values_mut() {
            *position = start_index;
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

    /// Snapshot-Iterator über den aktuellen Inhalt (keine Live-Änderungen).
    pub fn iter(&self) -> RingBufferIter {
        let inner = self.inner.lock().unwrap();
        let snapshot: Vec<PcmFrame> = inner.buffer.iter().cloned().collect();
        RingBufferIter {
            buffer: snapshot,
            index: 0,
        }
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
