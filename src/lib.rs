// src/lib.rs
pub mod api;
pub mod app;
pub mod audio;
pub mod codecs;
pub mod config;
pub mod core;
pub mod decoders;
pub mod encoders;
pub mod processors;
pub mod producers;
pub mod ring;
pub mod testing;
pub mod types;
pub mod monitoring;

// Re-export die wichtigsten Typen
pub use core::timestamp::utc_ns_now;
pub use core::{AirliftNode, AudioRingBuffer, ComponentLogger, Flow, LogContext};
pub use types::PcmFrame;
