// src/ring/audio_ring.rs
#![allow(dead_code)]

use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone)]
pub struct AudioSlot {
    pub seq: u64,
    pub utc_ns: u64,
    pub pcm: Arc<Vec<i16>>, // interleaved stereo
}

struct Inner {
    cap: usize,
    slots: Vec<AudioSlot>,
    head_seq: u64, // last written seq
}

#[derive(Clone)]
pub struct AudioRing {
    inner: Arc<Mutex<Inner>>,
    next_seq: Arc<AtomicU64>,
}

#[derive(Clone)]
pub struct RingReader {
    ring: AudioRing,
    last_seq: u64,
}

pub enum RingRead {
    Chunk(AudioSlot),
    Gap { missed: u64 }, // reader wurde überholt
    Empty,
}

impl AudioRing {
    pub fn new(cap: usize, prealloc_samples: usize) -> Self {
        // Preallocate all slots with empty vectors
        let mut slots = Vec::with_capacity(cap);
        for _i in 0..cap {
            slots.push(AudioSlot {
                seq: 0,
                utc_ns: 0,
                pcm: Arc::new(vec![0i16; prealloc_samples]),
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
        }
    }

    pub fn writer_push(&self, utc_ns: u64, pcm: Vec<i16>) -> u64 {
        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
        
        let mut g = self.inner.lock().unwrap();
        let idx = (seq as usize) % g.cap;
        
        // Get mutable access to the slot's PCM
        if let Some(existing_vec) = Arc::get_mut(&mut g.slots[idx].pcm) {
            existing_vec.clear();
            existing_vec.extend_from_slice(&pcm);
        } else {
            // Fallback: create new Arc if we can't get mutable access
            g.slots[idx].pcm = Arc::new(pcm);
        }
        
        g.slots[idx].seq = seq;
        g.slots[idx].utc_ns = utc_ns;
        g.head_seq = seq;
        
        seq
    }

    pub fn subscribe(&self) -> RingReader {
        let head = self.head_seq();
        RingReader { 
            ring: self.clone(), 
            last_seq: if head > 0 { head } else { 0 }
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
        }
    }
}

impl RingReader {
    pub fn poll(&mut self) -> RingRead {
        let head = self.ring.head_seq();
        if head == 0 || head <= self.last_seq {
            return RingRead::Empty;
        }

        let next = self.last_seq + 1;

        // Überholt? (writer hat mehr als cap weitergeschrieben)
        let cap = self.ring.cap() as u64;
        if head.saturating_sub(next) >= cap {
            let missed = head.saturating_sub(next) + 1 - cap;
            // springe auf "gerade noch im Ring"
            self.last_seq = head.saturating_sub(cap - 1);
            return RingRead::Gap { missed };
        }

        match self.ring.get_by_seq(next) {
            Some(slot) => {
                self.last_seq = next;
                RingRead::Chunk(slot)
            }
            None => RingRead::Empty, // kurz Race/Overwrite; poll später nochmal
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
    
    /// Letzte gelesene Sequenznummer
    pub fn last_seq(&self) -> u64 {
        self.last_seq
    }

    /// Aktuelle Head-Sequenz des Rings
    pub fn head_seq(&self) -> u64 {
        self.ring.head_seq()
    }

    /// Wie viele Slots liegt der Reader hinter dem Writer?
    pub fn fill(&self) -> u64 {
        let head = self.ring.head_seq();
        head.saturating_sub(self.last_seq)
    }
}

#[derive(Debug, Clone)]
pub struct RingStats {
    pub capacity: usize,
    pub head_seq: u64,
    pub next_seq: u64,
}
