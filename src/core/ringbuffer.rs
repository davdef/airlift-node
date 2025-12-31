use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use std::fmt::Debug;

use crate::core::lock::lock_mutex_with_timeout;
use crate::core::logging::ComponentLogger;
pub use crate::ring::PcmFrame;
use crate::ring::PcmSink;

#[derive(Debug)]
struct RingSlot {
    seq: AtomicU64,
    frame: Mutex<Option<PcmFrame>>,
}

#[derive(Debug)]
pub struct AudioRingBuffer {
    slots: Arc<Vec<RingSlot>>,
    capacity: usize,
    next_seq: AtomicU64,
    head_seq: AtomicU64,
    read_positions: Mutex<HashMap<String, u64>>,
    dropped_frames: AtomicU64,
    high_water_warned: AtomicBool,
}

const BUFFER_LOCK_TIMEOUT: Duration = Duration::from_millis(5);
const HIGH_WATER_THRESHOLD: f32 = 0.8;
const HIGH_WATER_RESET_THRESHOLD: f32 = 0.5;
const DROP_LOG_INTERVAL: u64 = 1_000;

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
            high_water_warned: AtomicBool::new(false),
        }
    }

    /// Push a frame into the ring.
    /// Returns the current number of frames in the buffer.
    pub fn push(&self, frame: PcmFrame) -> u64 {
        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);

        // Logging: Nur alle 50 Frames oder wenn interessant
        if seq % 50 == 0 || seq <= 5 {
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

        if let Some(mut guard) =
            lock_mutex_with_timeout(&slot.frame, "ringbuffer.push.slot", BUFFER_LOCK_TIMEOUT)
        {
            *guard = Some(frame);
        } else {
            self.dropped_frames.fetch_add(1, Ordering::Relaxed);
            self.warn("Dropping frame: slot lock timeout");
            return self.len() as u64;
        }

        slot.seq.store(seq, Ordering::Release);
        self.head_seq.store(seq, Ordering::Release);

        if seq > self.capacity as u64 {
            let dropped = self.dropped_frames.fetch_add(1, Ordering::Relaxed) + 1;
            if dropped == 1 || dropped % DROP_LOG_INTERVAL == 0 {
                self.debug(&format!(
                    "Frame dropped (ring overwrite). Total dropped: {}",
                    dropped
                ));
            }
        }

        let new_len = self.len() as u64;

        let utilization = new_len as f32 / self.capacity as f32;
        if utilization > HIGH_WATER_THRESHOLD {
            if !self.high_water_warned.swap(true, Ordering::Relaxed) {
                self.debug(&format!(
                    "Buffer high-water mark reached: {}/{}",
                    new_len, self.capacity
                ));
            }
        } else if utilization < HIGH_WATER_RESET_THRESHOLD {
            self.high_water_warned.store(false, Ordering::Relaxed);
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
        let target_seq = {
            let mut read_positions: MutexGuard<'_, HashMap<String, u64>> =
              match lock_mutex_with_timeout(
                &self.read_positions,
                "ringbuffer.pop.read_positions",
                BUFFER_LOCK_TIMEOUT,
            ) {
                Some(guard) => guard,
                None => {
                    self.warn("Pop aborted: read_positions lock timeout");
                    return None;
                }
            };
            let position = read_positions
                .entry(reader_id.to_string())
                .or_insert(oldest);
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
            let mut read_positions: MutexGuard<'_, HashMap<String, u64>> =
              match lock_mutex_with_timeout(
                &self.read_positions,
                "ringbuffer.pop.sequence_mismatch.read_positions",
                BUFFER_LOCK_TIMEOUT,
            ) {
                Some(guard) => guard,
                None => {
                    self.warn("Sequence mismatch handling skipped: read_positions lock timeout");
                    return None;
                }
            };
            if let Some(pos) = read_positions.get_mut(reader_id) {
                *pos = oldest;
            }

            self.warn(&format!(
                "Sequence mismatch for reader '{}': expected {}, got {}",
                reader_id, target_seq, slot_seq
            ));
            return None;
        }

        let frame = match lock_mutex_with_timeout(
            &slot.frame,
            "ringbuffer.pop.slot",
            BUFFER_LOCK_TIMEOUT,
        ) {
Some(guard) => {
    let guard: std::sync::MutexGuard<'_, Option<PcmFrame>> = guard;
    guard.as_ref().cloned()
}
            None => {
                self.warn("Pop aborted: slot lock timeout");
                return None;
            }
        };
        if frame.is_some() {
            let mut read_positions: MutexGuard<'_, HashMap<String, u64>> =
              match lock_mutex_with_timeout(
                &self.read_positions,
                "ringbuffer.pop.advance.read_positions",
                BUFFER_LOCK_TIMEOUT,
            ) {
                Some(guard) => guard,
                None => {
                    self.warn("Pop aborted: read_positions lock timeout");
                    return None;
                }
            };
            if let Some(pos) = read_positions.get_mut(reader_id) {
                *pos = target_seq + 1;
            }

            // Debug logging für interessante Frames
            if target_seq % 100 == 0 {
                self.debug(&format!(
                    "pop[reader={}] seq={} samples={}",
                    reader_id,
                    target_seq,
                    frame.as_ref().unwrap().samples.len()
                ));
            }
        }

        frame
    }

    pub fn clear(&self) {
        self.info("Clearing buffer");

        for slot in self.slots.iter() {
            if let Some(mut guard) =
                lock_mutex_with_timeout(&slot.frame, "ringbuffer.clear.slot", BUFFER_LOCK_TIMEOUT)
            {
                *guard = None;
            } else {
                self.warn("Clear aborted: slot lock timeout");
                return;
            }
            slot.seq.store(0, Ordering::Release);
        }

        self.head_seq.store(0, Ordering::Release);
        self.next_seq.store(1, Ordering::Release);
        self.dropped_frames.store(0, Ordering::Relaxed);

if let Some(mut read_positions) = lock_mutex_with_timeout(
    &self.read_positions,
    "ringbuffer.clear.read_positions",
    BUFFER_LOCK_TIMEOUT,
) {
    read_positions.clear();
} else {
    self.warn("Clear aborted: read_positions lock timeout");
}
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

let read_positions: MutexGuard<'_, HashMap<String, u64>> =
    match lock_mutex_with_timeout(
        &self.read_positions,
        "ringbuffer.available.read_positions",
        BUFFER_LOCK_TIMEOUT,
    ) {
        Some(guard) => guard,
        None => {
            self.warn("available_for_reader aborted: read_positions lock timeout");
            return 0;
        }
    };
        let reader_pos = read_positions.get(reader_id).copied().unwrap_or(oldest);

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
        if slot.seq.load(Ordering::Acquire) == seq {
            lock_mutex_with_timeout(
                &slot.frame,
                "ringbuffer.read_by_seq.slot",
                BUFFER_LOCK_TIMEOUT,
            )
            .and_then(|guard: std::sync::MutexGuard<'_, Option<PcmFrame>>| guard.clone())
        } else {
            None
        }
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
