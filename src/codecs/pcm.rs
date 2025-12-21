use anyhow::{anyhow, Result};

use crate::codecs::{
    AudioCodec, CodecInfo, CodecKind, ContainerKind, EncodedFrame, PCM_CHANNELS,
    PCM_I16_SAMPLES, PCM_SAMPLE_RATE,
};

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
