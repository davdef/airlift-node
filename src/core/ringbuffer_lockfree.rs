use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use crate::core::lock::{lock_rwlock_read_with_timeout, lock_rwlock_write_with_timeout};
use crate::core::logging::ComponentLogger;
pub use crate::ring::PcmFrame;
use crate::ring::PcmSink;

#[derive(Debug)]
struct RingSlot {
    seq: AtomicU64,
    frame: RwLock<Option<PcmFrame>>,
}

#[derive(Debug)]
struct ReaderSlot {
    id_hash: AtomicU64,
    position: AtomicU64,
}

impl ReaderSlot {
    fn new() -> Self {
        Self {
            id_hash: AtomicU64::new(0),
            position: AtomicU64::new(0),
        }
    }
}

#[derive(Debug)]
struct ReaderRegistry {
    slots: Vec<ReaderSlot>,
}

impl ReaderRegistry {
    fn new(max_readers: usize) -> Self {
        let mut slots = Vec::with_capacity(max_readers);
        for _ in 0..max_readers {
            slots.push(ReaderSlot::new());
        }
        Self { slots }
    }

    fn slot_for(&self, reader_id: &str) -> Option<&ReaderSlot> {
        let hash = hash_reader_id(reader_id);
        let slots_len = self.slots.len();
        let start = (hash as usize) % slots_len;

        for offset in 0..slots_len {
            let idx = (start + offset) % slots_len;
            let slot = &self.slots[idx];
            let slot_hash = slot.id_hash.load(Ordering::Acquire);
            if slot_hash == hash {
                return Some(slot);
            }
            if slot_hash == 0 {
                if slot
                    .id_hash
                    .compare_exchange(0, hash, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
                {
                    return Some(slot);
                }
                if slot.id_hash.load(Ordering::Acquire) == hash {
                    return Some(slot);
                }
            }
        }
        None
    }

    fn clear_positions(&self) {
        for slot in &self.slots {
            slot.position.store(0, Ordering::Release);
        }
    }
}

const MAX_READERS: usize = 64;
// Log interval/threshold constants for buffer diagnostics.
const LOG_EVERY_N_PUSH: u64 = 50;
const LOG_INITIAL_PUSH_COUNT: u64 = 5;
const LOG_EVERY_N_POP: u64 = 100;
const BUFFER_WARN_THRESHOLD: f32 = 0.8;
const BUFFER_LOCK_TIMEOUT: Duration = Duration::from_millis(5);

pub struct AudioRingBuffer {
    slots: Arc<Vec<RingSlot>>,
    capacity: usize,
    next_seq: AtomicU64,
    head_seq: AtomicU64,
    readers: ReaderRegistry,
    dropped_frames: AtomicU64,
}

impl AudioRingBuffer {
    pub fn new(capacity: usize) -> Self {
        let mut slots = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            slots.push(RingSlot {
                seq: AtomicU64::new(0),
                frame: RwLock::new(None),
            });
        }

        Self {
            slots: Arc::new(slots),
            capacity,
            next_seq: AtomicU64::new(1),
            head_seq: AtomicU64::new(0),
            readers: ReaderRegistry::new(MAX_READERS),
            dropped_frames: AtomicU64::new(0),
        }
    }

    /// Push a frame into the ring.
    /// Returns the current number of frames in the buffer.
    pub fn push(&self, frame: PcmFrame) -> u64 {
        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);

        if seq % LOG_EVERY_N_PUSH == 0 || seq <= LOG_INITIAL_PUSH_COUNT {
            self.debug(&format!(
                "push[seq={}] samples={} rate={} ch={}",
                seq,
                frame.samples.len(),
                frame.sample_rate,
                frame.channels
            ));
        }

        let idx = (seq as usize) % self.capacity;
        let slot = &self.slots[idx];

        if let Some(mut guard) = lock_rwlock_write_with_timeout(
            &slot.frame,
            "ringbuffer_lockfree.push.slot",
            BUFFER_LOCK_TIMEOUT,
        ) {
            *guard = Some(frame);
            slot.seq.store(seq, Ordering::Release);
        } else {
            self.dropped_frames.fetch_add(1, Ordering::Relaxed);
            self.warn("Dropping frame: slot write lock timeout");
            return self.len() as u64;
        }
        self.head_seq.store(seq, Ordering::Release);

        if seq > self.capacity as u64 {
            self.dropped_frames.fetch_add(1, Ordering::Relaxed);
            self.warn(&format!(
                "Frame dropped! Total dropped: {}",
                self.dropped_frames.load(Ordering::Relaxed)
            ));
        }

        let new_len = self.len() as u64;
        if new_len as f32 / self.capacity as f32 > BUFFER_WARN_THRESHOLD {
            self.warn(&format!("Buffer >80% full: {}/{}", new_len, self.capacity));
        }

        new_len
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
        let reader_slot = match self.readers.slot_for(reader_id) {
            Some(slot) => slot,
            None => {
                self.warn(&format!("No reader slot available for '{}'", reader_id));
                return None;
            }
        };

        let mut position = reader_slot.position.load(Ordering::Acquire);
        if position == 0 {
            let _ = reader_slot.position.compare_exchange(
                0,
                oldest,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            position = reader_slot.position.load(Ordering::Acquire);
        }
        if position < oldest {
            reader_slot.position.store(oldest, Ordering::Release);
            position = oldest;
        }
        if position > head {
            return None;
        }

        let slot = &self.slots[(position as usize) % self.capacity];
        let guard = match lock_rwlock_read_with_timeout(
            &slot.frame,
            "ringbuffer_lockfree.pop.slot",
            BUFFER_LOCK_TIMEOUT,
        ) {
            Some(guard) => guard,
            None => {
                self.warn("Pop aborted: slot read lock timeout");
                return None;
            }
        };
        let slot_seq = slot.seq.load(Ordering::Acquire);
        if slot_seq != position {
            self.dropped_frames.fetch_add(1, Ordering::Relaxed);
            reader_slot.position.store(oldest, Ordering::Release);
            self.warn(&format!(
                "Sequence mismatch for reader '{}': expected {}, got {}",
                reader_id, position, slot_seq
            ));
            return None;
        }

        let frame = match guard.as_ref() {
            Some(frame) => frame.clone(),
            None => return None,
        };

        reader_slot.position.store(position + 1, Ordering::Release);

        if position % LOG_EVERY_N_POP == 0 {
            self.debug(&format!(
                "pop[reader={}] seq={} samples={}",
                reader_id,
                position,
                frame.samples.len()
            ));
        }

        Some(frame)
    }

    pub fn clear(&self) {
        self.info("Clearing buffer");

        for slot in self.slots.iter() {
            if let Some(mut guard) = lock_rwlock_write_with_timeout(
                &slot.frame,
                "ringbuffer_lockfree.clear.slot",
                BUFFER_LOCK_TIMEOUT,
            ) {
                *guard = None;
            } else {
                self.warn("Clear aborted: slot write lock timeout");
                return;
            }
            slot.seq.store(0, Ordering::Release);
        }

        self.head_seq.store(0, Ordering::Release);
        self.next_seq.store(1, Ordering::Release);
        self.dropped_frames.store(0, Ordering::Relaxed);
        self.readers.clear_positions();
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

    /// Anzahl der für einen bestimmten Reader verfügbaren Frames
    pub fn available_for_reader(&self, reader_id: &str) -> usize {
        let head = self.head_seq.load(Ordering::Acquire);
        if head == 0 {
            return 0;
        }

        let oldest = self.oldest_seq(head);
        let reader_slot = match self.readers.slot_for(reader_id) {
            Some(slot) => slot,
            None => return 0,
        };
        let mut reader_pos = reader_slot.position.load(Ordering::Acquire);
        if reader_pos == 0 {
            let _ = reader_slot.position.compare_exchange(
                0,
                oldest,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            reader_pos = reader_slot.position.load(Ordering::Acquire);
        }
        if reader_pos < oldest {
            reader_slot.position.store(oldest, Ordering::Release);
            reader_pos = oldest;
        }

        if reader_pos > head {
            0
        } else {
            (head - reader_pos + 1) as usize
        }
    }

    /// Anzahl der für den "default" Reader verfügbaren Frames
    pub fn available(&self) -> usize {
        self.available_for_reader("default")
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

        RingBufferIter {
            buffer: snapshot,
            index: 0,
        }
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
        let guard = match lock_rwlock_read_with_timeout(
            &slot.frame,
            "ringbuffer_lockfree.read_by_seq.slot",
            BUFFER_LOCK_TIMEOUT,
        ) {
            Some(guard) => guard,
            None => {
                self.warn("Read aborted: slot read lock timeout");
                return None;
            }
        };
        let seq_start = slot.seq.load(Ordering::Acquire);
        if seq_start != seq {
            return None;
        }
        guard.as_ref().cloned()
    }

    fn slot_timestamp(&self, seq: u64) -> Option<u64> {
        self.read_by_seq(seq).map(|frame| frame.utc_ns)
    }
}

impl PcmSink for AudioRingBuffer {
    fn push(&self, frame: PcmFrame) -> anyhow::Result<()> {
        self.push(frame);
        Ok(())
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

// Implementierung des ComponentLogger Traits für AudioRingBuffer
impl crate::core::logging::ComponentLogger for AudioRingBuffer {
    fn log_context(&self) -> crate::core::logging::LogContext {
        crate::core::logging::LogContext::new("RingBuffer", &format!("{:p}", self as *const _))
    }
}

fn hash_reader_id(reader_id: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    reader_id.hash(&mut hasher);
    let hash = hasher.finish();
    if hash == 0 {
        1
    } else {
        hash
    }
}
