use anyhow::{anyhow, Result};
use vorbis::{Encoder, Quality};

pub struct VorbisEncoder {
    enc: Encoder,
    buffer: Vec<u8>,
}

impl VorbisEncoder {
    pub fn new(quality: f32) -> Result<Self> {
        let mut enc = Encoder::new()?;
        enc.set_quality(Quality::new(quality))?;
        enc.set_sample_rate(48000)?;
        enc.set_channels(2)?;

        Ok(Self {
            enc,
            buffer: Vec::new(),
        })
    }

    pub fn encode_100ms(&mut self, pcm: &[i16]) -> Result<Vec<u8>> {
        const SAMPLES_PER_100MS: usize = 4800 * 2; // 48kHz * 0.1s * stereo

        if pcm.len() != SAMPLES_PER_100MS {
            return Err(anyhow!("Wrong PCM length for 100ms"));
        }

        self.buffer.clear();

        // PCM zu floats konvertieren (Vorbis benötigt f32)
        let mut floats = Vec::with_capacity(pcm.len());
        for &sample in pcm {
            floats.push(sample as f32 / 32768.0);
        }

        // Encode
        let encoded = self.enc.encode(&floats)?;
        self.buffer.extend_from_slice(&encoded);

        Ok(self.buffer.clone())
    }
}
