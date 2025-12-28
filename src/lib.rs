// src/lib.rs
pub mod config;
pub mod core;
pub mod producers;
pub mod processors;

// Re-export die wichtigsten Typen
pub use core::{AirliftNode, Flow, AudioRingBuffer, ComponentLogger, LogContext};
pub use core::ringbuffer::PcmFrame;
pub use core::timestamp::utc_ns_now;
