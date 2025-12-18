// src/agent/mod.rs
use crate::ring::AudioRing;

pub struct Agent {
    pub ring: AudioRing,
}

impl Agent {
    pub fn new(cap_slots: usize, prealloc_samples: usize) -> Self {
        Self { 
            ring: AudioRing::new(cap_slots, prealloc_samples) 
        }
    }
}
