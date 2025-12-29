use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::ring::{PcmFrame, PcmSink};

#[derive(Clone)]
pub struct AudioSlot {
    pub seq: u64,
    pub utc_ns: u64,
    pub sample_rate: u32,
    pub channels: u8,
    pub samples: Arc<Vec<i16>>,
}

struct Inner {
    cap: usize,
    slots: Vec<AudioSlot>,
    head_seq: u64,
}

#[derive(Clone)]
pub struct AudioRing {
    inner: Arc<Mutex<Inner>>,
    next_seq: Arc<AtomicU64>,
    arc_replacements: Arc<AtomicU64>,
}

#[derive(Clone)]
pub struct RingReader {
    ring: AudioRing,
    last_seq: u64,
}

pub enum RingRead {
    Chunk(AudioSlot),
    Gap { missed: u64 },
    Empty,
}

impl AudioRing {
    pub fn new(cap: usize, prealloc_samples: usize, sample_rate: u32, channels: u8) -> Self {
        let mut slots = Vec::with_capacity(cap);
        for _ in 0..cap {
            slots.push(AudioSlot {
                seq: 0,
                utc_ns: 0,
                sample_rate,
                channels,
                samples: Arc::new(vec![0i16; prealloc_samples]),
            });
        }

        let inner = Inner {
            cap,
            slots,
            head_seq: 0,
        };

        Self {
            inner: Arc::new(Mutex::new(inner)),
            next_seq: Arc::new(AtomicU64::new(1)),
            arc_replacements: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn writer_push(&self, frame: PcmFrame) -> u64 {
        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);

        let mut g = self.inner.lock().unwrap();
        let idx = (seq as usize) % g.cap;

        if let Some(existing_vec) = Arc::get_mut(&mut g.slots[idx].samples) {
            existing_vec.clear();
            existing_vec.extend_from_slice(&frame.samples);
        } else {
            self.arc_replacements.fetch_add(1, Ordering::Relaxed);
            g.slots[idx].samples = Arc::new(frame.samples);
        }

        g.slots[idx].seq = seq;
        g.slots[idx].utc_ns = frame.utc_ns;
        g.slots[idx].sample_rate = frame.sample_rate;
        g.slots[idx].channels = frame.channels;
        g.head_seq = seq;

        seq
    }

    pub fn subscribe(&self) -> RingReader {
        let head = self.head_seq();
        RingReader {
            ring: self.clone(),
            last_seq: if head > 0 { head } else { 0 },
        }
    }

    pub fn head_seq(&self) -> u64 {
        let g = self.inner.lock().unwrap();
        g.head_seq
    }

    fn get_by_seq(&self, seq: u64) -> Option<AudioSlot> {
        let g = self.inner.lock().unwrap();
        let idx = (seq as usize) % g.cap;
        let slot = &g.slots[idx];

        if slot.seq == seq && seq > 0 {
            Some(slot.clone())
        } else {
            None
        }
    }

    fn cap(&self) -> usize {
        let g = self.inner.lock().unwrap();
        g.cap
    }

    pub fn stats(&self) -> RingStats {
        let g = self.inner.lock().unwrap();
        RingStats {
            capacity: g.cap,
            head_seq: g.head_seq,
            next_seq: self.next_seq.load(Ordering::Relaxed),
            arc_replacements: self.arc_replacements.load(Ordering::Relaxed),
        }
    }
}

impl PcmSink for AudioRing {
    fn push(&self, frame: PcmFrame) -> anyhow::Result<()> {
        self.writer_push(frame);
        Ok(())
    }
}

impl RingReader {
    pub fn poll(&mut self) -> RingRead {
        let head = self.ring.head_seq();
        if head == 0 || head <= self.last_seq {
            return RingRead::Empty;
        }

        let next = self.last_seq + 1;
        let cap = self.ring.cap() as u64;
        if head.saturating_sub(next) >= cap {
            let missed = head.saturating_sub(next) + 1 - cap;
            self.last_seq = head.saturating_sub(cap - 1);
            return RingRead::Gap { missed };
        }

        match self.ring.get_by_seq(next) {
            Some(slot) => {
                self.last_seq = next;
                RingRead::Chunk(slot)
            }
            None => RingRead::Empty,
        }
    }

    pub fn wait_for_data(&mut self) {
        loop {
            match self.poll() {
                RingRead::Chunk(_) => break,
                _ => std::thread::sleep(std::time::Duration::from_millis(5)),
            }
        }
    }

    pub fn last_seq(&self) -> u64 {
        self.last_seq
    }

    pub fn head_seq(&self) -> u64 {
        self.ring.head_seq()
    }

    pub fn fill(&self) -> u64 {
        let head = self.ring.head_seq();
        head.saturating_sub(self.last_seq)
    }

    pub fn follow(&mut self) {
        let head = self.ring.head_seq();
        self.last_seq = head;
    }
}

#[derive(Debug, Clone)]
pub struct RingStats {
    pub capacity: usize,
    pub head_seq: u64,
    pub next_seq: u64,
    pub arc_replacements: u64,
}
