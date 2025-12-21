use anyhow::{anyhow, Result};
use vorbis::{Encoder as VorbisEnc, VorbisQuality};

use crate::codecs::{
    AudioCodec, CodecInfo, CodecKind, ContainerKind, EncodedFrame, PCM_CHANNELS,
    PCM_I16_SAMPLES, PCM_SAMPLE_RATE,
};

pub struct VorbisEncoder {
    enc: VorbisEnc,
    buffer: Vec<u8>,
    info: CodecInfo,
}

impl VorbisEncoder {
    pub fn new(_quality: f32) -> Result<Self> {
        // vorbis 0.1 kennt nur feste Presets
        let enc = VorbisEnc::new(2, PCM_SAMPLE_RATE, VorbisQuality::Quality)
            .map_err(|e| anyhow!("vorbis init failed: {:?}", e))?;

        Ok(Self {
            enc,
            buffer: Vec::with_capacity(64 * 1024),
            info: CodecInfo {
                kind: CodecKind::Vorbis,
                sample_rate: PCM_SAMPLE_RATE,
                channels: PCM_CHANNELS,
                container: ContainerKind::Ogg,
            },
        })
    }

    pub fn encode_100ms(&mut self, pcm: &[i16]) -> Result<Vec<u8>> {
        if pcm.len() != PCM_I16_SAMPLES {
            return Err(anyhow!(
                "VorbisEncoder: expected {} samples, got {}",
                PCM_I16_SAMPLES,
                pcm.len()
            ));
        }

        self.buffer.clear();

        let encoded = self
            .enc
            .encode(&pcm.to_vec())
            .map_err(|e| anyhow!("vorbis encode failed: {:?}", e))?;

        self.buffer.extend_from_slice(&encoded);
        Ok(self.buffer.clone())
    }
}

impl AudioCodec for VorbisEncoder {
    fn info(&self) -> &CodecInfo {
        &self.info
    }

    fn encode(&mut self, pcm: &[i16]) -> Result<Vec<EncodedFrame>> {
        let payload = self.encode_100ms(pcm)?;
        if payload.is_empty() {
            return Ok(Vec::new());
        }

        Ok(vec![EncodedFrame {
            payload,
            info: self.info.clone(),
        }])
    }
}
