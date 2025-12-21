use anyhow::{anyhow, Result};
use vorbis::{Encoder as VorbisEnc, VorbisQuality};

pub struct VorbisEncoder {
    enc: VorbisEnc,
    buffer: Vec<u8>,
}

impl VorbisEncoder {
    pub fn new(_quality: f32) -> Result<Self> {
        // vorbis 0.1 kennt nur feste Presets
        let enc = VorbisEnc::new(2, 48_000, VorbisQuality::Quality)
            .map_err(|e| anyhow!("vorbis init failed: {:?}", e))?;

        Ok(Self {
            enc,
            buffer: Vec::with_capacity(64 * 1024),
        })
    }

    pub fn encode_100ms(&mut self, pcm: &[i16]) -> Result<Vec<u8>> {
        // 100 ms @ 48 kHz stereo = 4800 Frames = 9600 i16
        const SAMPLES_100MS: usize = 4_800 * 2;

        if pcm.len() != SAMPLES_100MS {
            return Err(anyhow!(
                "VorbisEncoder: expected {} samples, got {}",
                SAMPLES_100MS,
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
