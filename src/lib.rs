// src/lib.rs
pub mod config;
pub mod codecs;
pub mod core;
pub mod decoders;
pub mod encoders;
pub mod audio;
pub mod producers;
pub mod processors;
pub mod ring;
pub mod types;

// Re-export die wichtigsten Typen
pub use core::{AirliftNode, Flow, AudioRingBuffer, ComponentLogger, LogContext};
pub use types::PcmFrame;
pub use core::timestamp::utc_ns_now;
