use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct PcmFrame {
    pub utc_ns: u64,
    pub samples: Vec<i16>,
    pub sample_rate: u32,
    pub channels: u8,
}

#[derive(Debug)]
struct RingSlot {
    seq: AtomicU64,
    frame: Mutex<Option<PcmFrame>>,
}

pub struct AudioRingBuffer {
    slots: Arc<Vec<RingSlot>>,
    capacity: usize,
    next_seq: AtomicU64,
    head_seq: AtomicU64,
    read_positions: Mutex<HashMap<String, u64>>,
    dropped_frames: AtomicU64,
}

impl AudioRingBuffer {
    pub fn new(capacity: usize) -> Self {
        let mut slots = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            slots.push(RingSlot {
                seq: AtomicU64::new(0),
                frame: Mutex::new(None),
            });
        }

        Self {
            slots: Arc::new(slots),
            capacity,
            next_seq: AtomicU64::new(1),
            head_seq: AtomicU64::new(0),
            read_positions: Mutex::new(HashMap::new()),
            dropped_frames: AtomicU64::new(0),
        }
    }

    /// Push a frame into the ring.
    /// Returns the current number of frames in the buffer.
    pub fn push(&self, frame: PcmFrame) -> u64 {
        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
        let idx = (seq as usize) % self.capacity;
        let slot = &self.slots[idx];

        {
            let mut guard = slot.frame.lock().unwrap();
            *guard = Some(frame);
        }

        slot.seq.store(seq, Ordering::Release);
        self.head_seq.store(seq, Ordering::Release);

        if seq > self.capacity as u64 {
            self.dropped_frames.fetch_add(1, Ordering::Relaxed);
        }

        self.len() as u64
    }

    pub fn pop(&self) -> Option<PcmFrame> {
        self.pop_for_reader("default")
    }

    /// Leser-spezifisches Pop, mit Reader-ID (Multi-Reader).
    pub fn pop_for_reader(&self, reader_id: &str) -> Option<PcmFrame> {
        let head = self.head_seq.load(Ordering::Acquire);
        if head == 0 {
            return None;
        }

        let oldest = self.oldest_seq(head);
        let target_seq = {
            let mut read_positions = self.read_positions.lock().unwrap();
            let position = read_positions.entry(reader_id.to_string()).or_insert(oldest);
            if *position < oldest {
                *position = oldest;
            }
            if *position > head {
                return None;
            }
            *position
        };

        let slot = &self.slots[(target_seq as usize) % self.capacity];
        let slot_seq = slot.seq.load(Ordering::Acquire);
        if slot_seq != target_seq {
            self.dropped_frames.fetch_add(1, Ordering::Relaxed);
            let mut read_positions = self.read_positions.lock().unwrap();
            if let Some(pos) = read_positions.get_mut(reader_id) {
                *pos = oldest;
            }
            return None;
        }

        let frame = slot.frame.lock().unwrap().clone();
        if frame.is_some() {
            let mut read_positions = self.read_positions.lock().unwrap();
            if let Some(pos) = read_positions.get_mut(reader_id) {
                *pos = target_seq + 1;
            }
        }

        frame
    }

    pub fn clear(&self) {
        for slot in self.slots.iter() {
            let mut guard = slot.frame.lock().unwrap();
            *guard = None;
            slot.seq.store(0, Ordering::Release);
        }

        self.head_seq.store(0, Ordering::Release);
        self.next_seq.store(1, Ordering::Release);
        self.dropped_frames.store(0, Ordering::Relaxed);

        let mut read_positions = self.read_positions.lock().unwrap();
        read_positions.clear();
    }

    pub fn len(&self) -> usize {
        let head = self.head_seq.load(Ordering::Acquire);
        if head == 0 {
            return 0;
        }
        let oldest = self.oldest_seq(head);
        (head - oldest + 1) as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn stats(&self) -> RingBufferStats {
        let head = self.head_seq.load(Ordering::Acquire);
        if head == 0 {
            return RingBufferStats {
                capacity: self.capacity,
                current_frames: 0,
                dropped_frames: self.dropped_frames.load(Ordering::Relaxed),
                latest_timestamp: None,
                oldest_timestamp: None,
            };
        }

        let oldest = self.oldest_seq(head);
        let latest_timestamp = self.slot_timestamp(head);
        let oldest_timestamp = self.slot_timestamp(oldest);

        RingBufferStats {
            capacity: self.capacity,
            current_frames: self.len(),
            dropped_frames: self.dropped_frames.load(Ordering::Relaxed),
            latest_timestamp,
            oldest_timestamp,
        }
    }

    /// Snapshot-Iterator über den aktuellen Inhalt (keine Live-Änderungen).
    pub fn iter(&self) -> RingBufferIter {
        let head = self.head_seq.load(Ordering::Acquire);
        if head == 0 {
            return RingBufferIter {
                buffer: Vec::new(),
                index: 0,
            };
        }

        let oldest = self.oldest_seq(head);
        let mut snapshot = Vec::with_capacity(self.len());
        for seq in oldest..=head {
            if let Some(frame) = self.read_by_seq(seq) {
                snapshot.push(frame);
            }
        }

        RingBufferIter { buffer: snapshot, index: 0 }
    }

    fn oldest_seq(&self, head: u64) -> u64 {
        if head >= self.capacity as u64 {
            head - self.capacity as u64 + 1
        } else {
            1
        }
    }

    fn read_by_seq(&self, seq: u64) -> Option<PcmFrame> {
        let slot = &self.slots[(seq as usize) % self.capacity];
        if slot.seq.load(Ordering::Acquire) == seq {
            slot.frame.lock().unwrap().clone()
        } else {
            None
        }
    }

    fn slot_timestamp(&self, seq: u64) -> Option<u64> {
        self.read_by_seq(seq).map(|frame| frame.utc_ns)
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
