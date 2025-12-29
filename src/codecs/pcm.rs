use anyhow::{anyhow, Result};

use crate::codecs::{
    AudioCodec, CodecInfo, CodecKind, ContainerKind, EncodedFrame, PCM_CHANNELS, PCM_I16_SAMPLES,
    PCM_SAMPLE_RATE,
};
use crate::decoders::AudioDecoder;
use crate::ring::PcmFrame;

pub struct PcmCodec {
    info: CodecInfo,
    buffer: Vec<u8>,
}

impl PcmCodec {
    pub fn new() -> Self {
        Self {
            info: CodecInfo {
                kind: CodecKind::Pcm,
                sample_rate: PCM_SAMPLE_RATE,
                channels: PCM_CHANNELS,
                container: ContainerKind::Raw,
            },
            buffer: Vec::with_capacity(PCM_I16_SAMPLES * 2),
        }
    }
}

pub struct PcmPassthroughDecoder {
    chunk_start_ns: u64,
    chunk_counter: u64,
    next_utc_ns: Option<u64>,
}

impl PcmPassthroughDecoder {
    pub fn new(chunk_start_ns: u64) -> Self {
        Self {
            chunk_start_ns,
            chunk_counter: 0,
            next_utc_ns: None,
        }
    }

    pub fn set_next_timestamp(&mut self, utc_ns: u64) {
        self.next_utc_ns = Some(utc_ns);
    }
}

impl AudioCodec for PcmCodec {
    fn info(&self) -> &CodecInfo {
        &self.info
    }

    fn encode(&mut self, pcm: &[i16]) -> Result<Vec<EncodedFrame>> {
        if pcm.len() != PCM_I16_SAMPLES {
            return Err(anyhow!(
                "PCM codec expected {} samples, got {}",
                PCM_I16_SAMPLES,
                pcm.len()
            ));
        }

        self.buffer.clear();
        self.buffer.reserve(pcm.len() * 2);
        for sample in pcm {
            self.buffer.extend_from_slice(&sample.to_le_bytes());
        }

        let payload = self.buffer.clone();
        Ok(vec![EncodedFrame {
            payload,
            info: self.info.clone(),
        }])
    }
}

impl AudioDecoder for PcmPassthroughDecoder {
    fn decode(&mut self, packet: &[u8]) -> Result<Option<PcmFrame>> {
        if packet.len() % 2 != 0 {
            return Err(anyhow!("invalid pcm payload length {}", packet.len()));
        }

        let samples = packet.len() / 2;
        let mut pcm = Vec::with_capacity(samples);
        for chunk in packet.chunks_exact(2) {
            pcm.push(i16::from_le_bytes([chunk[0], chunk[1]]));
        }

        if pcm.len() != PCM_I16_SAMPLES {
            return Err(anyhow!(
                "PCM passthrough expected {} samples, got {}",
                PCM_I16_SAMPLES,
                pcm.len()
            ));
        }

        let utc_ns = self
            .next_utc_ns
            .take()
            .unwrap_or(self.chunk_start_ns + (self.chunk_counter * 100_000_000));
        self.chunk_counter += 1;

        Ok(Some(PcmFrame {
            utc_ns,
            samples: pcm,
            sample_rate: PCM_SAMPLE_RATE,
            channels: PCM_CHANNELS,
        }))
    }
}
