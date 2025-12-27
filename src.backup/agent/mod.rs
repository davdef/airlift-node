// src/agent/mod.rs
use crate::ring::{AudioRing, EncodedRing};

pub struct Agent {
    pub ring: AudioRing,
    pub encoded_ring: EncodedRing,
}

impl Agent {
    pub fn new(cap_slots: usize, prealloc_samples: usize) -> Self {
        Self {
            ring: AudioRing::new(cap_slots, prealloc_samples),
            encoded_ring: EncodedRing::new(cap_slots),
        }
    }
}
