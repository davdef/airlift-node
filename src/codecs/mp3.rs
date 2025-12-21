use anyhow::{anyhow, Result};
use lame::Lame;

pub struct Mp3Encoder {
    lame: Lame,
    mp3_buffer: Vec<u8>,
    left: Vec<i16>,
    right: Vec<i16>,
    frames_per_100ms: usize,
}

impl Mp3Encoder {
    pub fn new(bitrate: u32, sample_rate: u32) -> Result<Self> {
        const MP3_BUFFER_OVERHEAD: usize = 7200;
        let frames_per_100ms = (sample_rate / 10) as usize;
        let mp3_buffer_size = frames_per_100ms * 5 / 4 + MP3_BUFFER_OVERHEAD;

        let mut lame = Lame::new().ok_or_else(|| anyhow!("Failed to init LAME"))?;

        // Korrektur: Kein Cast zu i32, da lame::Lame u32 erwartet
        lame.set_sample_rate(sample_rate)
            .map_err(|e| anyhow!("lame set_sample_rate: {:?}", e))?;
        lame.set_channels(2).map_err(|e| anyhow!("lame set_channels: {:?}", e))?;
        lame.set_kilobitrate(bitrate as i32)
            .map_err(|e| anyhow!("lame set_kilobitrate: {:?}", e))?;
        lame.set_quality(2).map_err(|e| anyhow!("lame set_quality: {:?}", e))?;
        lame.init_params().map_err(|e| anyhow!("lame init_params: {:?}", e))?;

        Ok(Self {
            lame,
            mp3_buffer: vec![0u8; mp3_buffer_size],
            left: vec![0i16; frames_per_100ms],
            right: vec![0i16; frames_per_100ms],
            frames_per_100ms,
        })
    }

    pub fn encode_100ms(&mut self, pcm: &[i16], out_buffer: &mut Vec<u8>) -> Result<usize> {
        let expected_len = self.frames_per_100ms * 2;
        if pcm.len() != expected_len {
            return Err(anyhow!(
                "Wrong PCM length for 100ms: expected {}, got {}",
                expected_len,
                pcm.len()
            ));
        }

        // PCM-Daten in separate Kanäle aufteilen
        // Sicherstellen, dass wir nicht über die Grenzen schreiben
        let frames_to_process = self.frames_per_100ms.min(pcm.len() / 2);

        for i in 0..frames_to_process {
            let idx = i * 2;
            self.left[i] = pcm[idx];
            self.right[i] = pcm[idx + 1];
        }

        // Bei unvollständigen Daten, Rest mit Nullen auffüllen
        if frames_to_process < self.frames_per_100ms {
            for i in frames_to_process..self.frames_per_100ms {
                self.left[i] = 0;
                self.right[i] = 0;
            }
        }

        // Enkodieren
        let encoded_bytes = self
            .lame
            .encode(&self.left, &self.right, &mut self.mp3_buffer)
            .map_err(|e| anyhow!("lame encode: {:?}", e))?;

        // Ergebnis in out_buffer kopieren (ohne to_vec-Allokation)
        out_buffer.clear();
        out_buffer.extend_from_slice(&self.mp3_buffer[..encoded_bytes]);

        Ok(encoded_bytes)
    }
}
