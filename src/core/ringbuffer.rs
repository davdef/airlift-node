use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::core::logging::ComponentLogger;
pub use crate::ring::PcmFrame;
use crate::ring::PcmSink;

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

        {
            let mut guard = slot.frame.lock().unwrap();
            *guard = Some(frame);
        }

        slot.seq.store(seq, Ordering::Release);
        self.head_seq.store(seq, Ordering::Release);

        if seq > self.capacity as u64 {
            self.dropped_frames.fetch_add(1, Ordering::Relaxed);
            self.warn(&format!("Frame dropped! Total dropped: {}", 
                self.dropped_frames.load(Ordering::Relaxed)));
        }

        let new_len = self.len() as u64;
        
        // Warnung bei hoher Auslastung
        if new_len as f32 / self.capacity as f32 > 0.8 {
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
            
            self.warn(&format!("Sequence mismatch for reader '{}': expected {}, got {}", 
                reader_id, target_seq, slot_seq));
            return None;
        }

        let frame = slot.frame.lock().unwrap().clone();
        if frame.is_some() {
            let mut read_positions = self.read_positions.lock().unwrap();
            if let Some(pos) = read_positions.get_mut(reader_id) {
                *pos = target_seq + 1;
            }
            
            // Debug logging für interessante Frames
            if target_seq % 100 == 0 {
                self.debug(&format!(
                    "pop[reader={}] seq={} samples={}",
                    reader_id, target_seq, 
                    frame.as_ref().unwrap().samples.len()
                ));
            }
        }

        frame
    }

    pub fn clear(&self) {
        self.info("Clearing buffer");
        
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

    /// Anzahl der für einen bestimmten Reader verfügbaren Frames
    pub fn available_for_reader(&self, reader_id: &str) -> usize {
        let head = self.head_seq.load(Ordering::Acquire);
        if head == 0 {
            return 0;
        }
        
        let oldest = self.oldest_seq(head);
        let read_positions = self.read_positions.lock().unwrap();
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

// Unit Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_buffer_operations_debug() {
        let buffer = AudioRingBuffer::new(10);
        assert_eq!(buffer.len(), 0);
        assert_eq!(buffer.available(), 0);
        
        let frame = PcmFrame {
            utc_ns: 123456789,
            samples: vec![1, 2, 3, 4, 5, 6],
            sample_rate: 48000,
            channels: 2,
        };
        
        let new_len = buffer.push(frame);
        assert_eq!(new_len, 1);
        assert_eq!(buffer.len(), 1);
        assert_eq!(buffer.available(), 1);
        
        let popped = buffer.pop();
        assert!(popped.is_some());
        
        // Frame ist noch im Buffer, aber nicht mehr verfügbar für "default"
        assert_eq!(buffer.len(), 1);
        assert_eq!(buffer.available(), 0);
        
        // Für anderen Reader verfügbar
        assert_eq!(buffer.available_for_reader("other"), 1);
    }

    #[test]
    fn test_basic_buffer_operations() {
        let buffer = AudioRingBuffer::new(10);
        assert_eq!(buffer.len(), 0);
        
        let frame = PcmFrame {
            utc_ns: 123456789,
            samples: vec![1, 2, 3, 4, 5, 6],
            sample_rate: 48000,
            channels: 2,
        };
        
        let new_len = buffer.push(frame);
        assert_eq!(new_len, 1);
        assert_eq!(buffer.len(), 1);
        
        // Reader "default" liest den Frame
        let popped = buffer.pop();
        assert!(popped.is_some());
        
        // Buffer hat immer noch 1 Frame (nur Leseposition geändert)
        assert_eq!(buffer.len(), 1);
        
        // Zweiter Versuch von "default" sollte nichts liefern
        let popped2 = buffer.pop();
        assert!(popped2.is_none());
        
        // Aber anderer Reader kann lesen
        let popped_by_other = buffer.pop_for_reader("other_reader");
        assert!(popped_by_other.is_some());
        
        // Jetzt haben beide Reader gelesen, Buffer immer noch da
        assert_eq!(buffer.len(), 1);
    }

    #[test]
    fn test_multi_reader() {
        let buffer = AudioRingBuffer::new(20);
        
        // Push 3 frames
        for i in 0..3 {
            let frame = PcmFrame {
                utc_ns: i as u64 * 1000,
                samples: vec![i as i16; 96],
                sample_rate: 48000,
                channels: 2,
            };
            buffer.push(frame);
        }
        
        assert_eq!(buffer.len(), 3);
        
        // Two different readers should both get the first frame
        let frame1 = buffer.pop_for_reader("reader1");
        let frame2 = buffer.pop_for_reader("reader2");
        
        assert!(frame1.is_some());
        assert!(frame2.is_some());
        assert_eq!(frame1.unwrap().samples[0], 0);
        assert_eq!(frame2.unwrap().samples[0], 0);
        
        // Now reader1 should get frame 1, reader2 should get frame 2
        let frame1_2 = buffer.pop_for_reader("reader1");
        let frame2_2 = buffer.pop_for_reader("reader2");
        
        assert!(frame1_2.is_some());
        assert!(frame2_2.is_some());
        assert_eq!(frame1_2.unwrap().samples[0], 1);
        assert_eq!(frame2_2.unwrap().samples[0], 1);
    }

    #[test]
    fn test_buffer_wrap_around() {
        let buffer = AudioRingBuffer::new(3);
        
        // Push mehr Frames als Capacity
        for i in 0..5 {
            let frame = PcmFrame {
                utc_ns: i as u64 * 1000,
                samples: vec![i as i16; 48],
                sample_rate: 48000,
                channels: 2,
            };
            buffer.push(frame);
        }
        
        // Should only have 3 frames (wrapped around)
        assert_eq!(buffer.len(), 3);
        
        let stats = buffer.stats();
        assert!(stats.dropped_frames > 0);
    }

    #[test]
    fn test_buffer_logging_integration() {
        use crate::core::logging::ComponentLogger;
        
        let buffer = AudioRingBuffer::new(5);
        
        // Teste, dass Logging-Methoden verfügbar sind
        buffer.debug("Test debug message");
        buffer.info("Test info message");
        buffer.warn("Test warning message");
        buffer.error("Test error message");
        
        // Teste buffer tracing
        buffer.trace_buffer(&buffer);
        
        // Fülle Buffer und trace
        for i in 0..3 {
            let frame = PcmFrame {
                utc_ns: i as u64 * 1000,
                samples: vec![i as i16; 48],
                sample_rate: 48000,
                channels: 2,
            };
            buffer.push(frame);
        }
        
        buffer.trace_buffer(&buffer);
        
        // Teste multi-reader mit logging
        let r1 = buffer.pop_for_reader("test_reader1");
        let r2 = buffer.pop_for_reader("test_reader2");
        
        assert!(r1.is_some());
        assert!(r2.is_some());
        
        buffer.trace_buffer(&buffer);
    }
}
