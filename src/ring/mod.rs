// src/ring/mod.rs
pub mod audio_ring;

// Re-export unter dem richtigen Namen
pub use audio_ring::AudioRing;
pub use audio_ring::RingReader;
pub use audio_ring::RingRead;
pub use audio_ring::RingStats;
pub use audio_ring::AudioSlot;
