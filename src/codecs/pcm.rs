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
        if cfg!(target_endian = "little") {
            self.buffer.extend_from_slice(bytemuck::cast_slice(pcm));
        } else {
            for sample in pcm {
                self.buffer.extend_from_slice(&sample.to_le_bytes());
            }
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

        let pcm = if cfg!(target_endian = "little") {
            let pcm_slice = bytemuck::try_cast_slice::<u8, i16>(packet).map_err(|_| {
                anyhow!("invalid pcm payload length {}", packet.len())
            })?;
            pcm_slice.to_vec()
        } else {
            let pcm_slice = bytemuck::try_cast_slice::<u8, i16>(packet).map_err(|_| {
                anyhow!("invalid pcm payload length {}", packet.len())
            })?;
            pcm_slice.iter().map(|sample| i16::from_le(*sample)).collect()
        };

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn pcm_roundtrip() {
        let mut codec = PcmCodec::new();
        let mut decoder = PcmPassthroughDecoder::new(0);
        let pcm: Vec<i16> = (0..PCM_I16_SAMPLES as i16).collect();

        let encoded = codec.encode(&pcm).expect("encode");
        let decoded = decoder
            .decode(&encoded[0].payload)
            .expect("decode")
            .expect("frame");

        assert_eq!(decoded.samples, pcm);
    }

    #[test]
    #[ignore]
    fn pcm_encode_decode_bench() {
        const ITERATIONS: usize = 5_000;
        let pcm: Vec<i16> = (0..PCM_I16_SAMPLES as i16).collect();

        let mut codec = PcmCodec::new();
        let mut decoder = PcmPassthroughDecoder::new(0);

        let start = Instant::now();
        for _ in 0..ITERATIONS {
            let encoded = codec.encode(&pcm).expect("encode");
            let _ = decoder
                .decode(&encoded[0].payload)
                .expect("decode")
                .expect("frame");
        }
        let direct_elapsed = start.elapsed();

        let start = Instant::now();
        for _ in 0..ITERATIONS {
            let mut buffer = Vec::with_capacity(pcm.len() * 2);
            for sample in &pcm {
                buffer.extend_from_slice(&sample.to_le_bytes());
            }
            for chunk in buffer.chunks_exact(2) {
                let _ = i16::from_le_bytes([chunk[0], chunk[1]]);
            }
        }
        let reference_elapsed = start.elapsed();

        eprintln!(
            "direct_cast: {:?}, reference: {:?}, ratio: {:.2}x",
            direct_elapsed,
            reference_elapsed,
            reference_elapsed.as_secs_f64() / direct_elapsed.as_secs_f64()
        );
    }
}
