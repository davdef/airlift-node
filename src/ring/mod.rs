pub mod audio_ring;
pub mod encoded_ring;

pub use audio_ring::AudioRing;
pub use audio_ring::AudioSlot;
pub use audio_ring::RingRead;
pub use audio_ring::RingReader;
pub use audio_ring::RingStats;
pub use encoded_ring::EncodedRing;
pub use encoded_ring::EncodedRingRead;
pub use encoded_ring::EncodedRingReader;

use crate::codecs::EncodedFrame;

#[derive(Clone, Debug)]
pub struct PcmFrame {
    pub utc_ns: u64,
    pub samples: Vec<i16>,
    pub sample_rate: u32,
    pub channels: u8,
}

#[derive(Clone, Debug)]
pub struct EncodedFramePacket {
    pub utc_ns: u64,
    pub frame: EncodedFrame,
}

pub trait PcmSink: Send + Sync {
    fn push(&self, frame: PcmFrame) -> anyhow::Result<()>;
}

pub trait EncodedSink: Send + Sync {
    fn push(&self, frame: EncodedFramePacket) -> anyhow::Result<()>;
}

pub trait EncodedSource: Send {
    fn poll(&mut self) -> EncodedRingRead;
    fn wait_for_read(&mut self) -> EncodedRingRead;
    fn wait_for_read_or_stop(
        &mut self,
        stop: &std::sync::atomic::AtomicBool,
    ) -> Option<EncodedRingRead>;
    fn notifier(&self) -> Option<std::sync::Arc<std::sync::Condvar>> {
        None
    }
}
