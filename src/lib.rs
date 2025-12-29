// src/lib.rs
pub mod config;
pub mod codecs;
pub mod core;
pub mod producers;
pub mod processors;
pub mod ring;

// Re-export die wichtigsten Typen
pub use core::{AirliftNode, Flow, AudioRingBuffer, ComponentLogger, LogContext};
pub use ring::PcmFrame;
pub use core::timestamp::utc_ns_now;
