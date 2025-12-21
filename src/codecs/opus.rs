use anyhow::{anyhow, Result};
use ogg::writing::PacketWriter;
use opus::{Application, Channels, Encoder};

use crate::codecs::{
    AudioCodec, CodecInfo, CodecKind, ContainerKind, EncodedFrame, PCM_CHANNELS,
    PCM_SAMPLE_RATE,
};

pub struct OggOpusEncoder {
    enc: Encoder,
    pw: PacketWriter<Vec<u8>>,
    serial: u32,
    gp: u64, // granule position (48k samples)
    info: CodecInfo,
}

impl OggOpusEncoder {
    pub fn new(bitrate: i32, vendor: &str) -> Result<Self> {
        Self::new_with_application(bitrate, vendor, Application::Audio)
    }

    pub fn new_with_application(
        bitrate: i32,
        vendor: &str,
        application: Application,
    ) -> Result<Self> {
        let mut enc = Encoder::new(PCM_SAMPLE_RATE, Channels::Stereo, application)?;
        enc.set_bitrate(opus::Bitrate::Bits(bitrate))?;

        let serial = rand::random::<u32>();
        let mut pw = PacketWriter::new(Vec::with_capacity(64 * 1024));

        // Wichtig: Head/Tags genau einmal am Anfang (BOS)
        pw.write_packet(
            opus_head().into(),
            serial,
            ogg::PacketWriteEndInfo::EndPage,
            0,
        )?;
        pw.write_packet(
            opus_tags(vendor).into(),
            serial,
            ogg::PacketWriteEndInfo::EndPage,
            0,
        )?;

        Ok(Self {
            enc,
            pw,
            serial,
            gp: 0,
            info: CodecInfo {
                kind: CodecKind::OpusOgg,
                sample_rate: PCM_SAMPLE_RATE,
                channels: PCM_CHANNELS,
                container: ContainerKind::Ogg,
            },
        })
    }

    pub fn encode_100ms(&mut self, pcm: &[i16]) -> Result<Vec<u8>> {
        // 100 ms @ 48 kHz stereo = 4800 Frames = 9600 i16 samples
        // Wir encoden in 20ms-Frames: 960 Frames pro Kanal => 960*2 i16 pro Frame
        const OPUS_FRAME_SAMPLES_PER_CH: usize = 960; // 20 ms @ 48k
        const CHANNELS: usize = 2;
        const OPUS_FRAME_I16: usize = OPUS_FRAME_SAMPLES_PER_CH * CHANNELS;

        if pcm.len() % OPUS_FRAME_I16 != 0 {
            return Err(anyhow!(
                "PCM len {} not multiple of {} (20ms stereo frame)",
                pcm.len(),
                OPUS_FRAME_I16
            ));
        }

        let mut opus_buf = [0u8; 4000];

        let mut frames = pcm.chunks_exact(OPUS_FRAME_I16);
        let n_frames = frames.len();
        if n_frames == 0 {
            return Ok(Vec::new());
        }
        let last = n_frames - 1;

        for (i, frame) in frames.by_ref().enumerate() {
            let n = self.enc.encode(frame, &mut opus_buf)?;

            // Granule = Ende dieses 20ms-Pakets (in 48k-Samples pro Kanal)
            self.gp += OPUS_FRAME_SAMPLES_PER_CH as u64;

            let end = if i == last {
                ogg::PacketWriteEndInfo::EndPage
            } else {
                ogg::PacketWriteEndInfo::NormalPacket
            };

            self.pw.write_packet(
                opus_buf[..n].to_vec().into_boxed_slice(),
                self.serial,
                end,
                self.gp,
            )?;
        }

        // KRITISCH: Writer NICHT ersetzen (sonst wieder BOS => Icecast meckert)
        // Stattdessen nur den inneren Vec leeren und rausholen:
        let mut out = Vec::new();
        std::mem::swap(&mut out, self.pw.inner_mut());
        Ok(out)
    }
}

impl AudioCodec for OggOpusEncoder {
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

pub struct OpusWebRtcEncoder {
    enc: Encoder,
    info: CodecInfo,
}

impl OpusWebRtcEncoder {
    pub fn new(bitrate: i32) -> Result<Self> {
        Self::new_with_application(bitrate, Application::Audio)
    }

    pub fn new_with_application(bitrate: i32, application: Application) -> Result<Self> {
        let mut enc = Encoder::new(PCM_SAMPLE_RATE, Channels::Stereo, application)?;
        enc.set_bitrate(opus::Bitrate::Bits(bitrate))?;

        Ok(Self {
            enc,
            info: CodecInfo {
                kind: CodecKind::OpusWebRtc,
                sample_rate: PCM_SAMPLE_RATE,
                channels: PCM_CHANNELS,
                container: ContainerKind::Rtp,
            },
        })
    }
}

impl AudioCodec for OpusWebRtcEncoder {
    fn info(&self) -> &CodecInfo {
        &self.info
    }

    fn encode(&mut self, pcm: &[i16]) -> Result<Vec<EncodedFrame>> {
        const OPUS_FRAME_SAMPLES_PER_CH: usize = 960; // 20 ms @ 48k
        const CHANNELS: usize = 2;
        const OPUS_FRAME_I16: usize = OPUS_FRAME_SAMPLES_PER_CH * CHANNELS;

        if pcm.len() % OPUS_FRAME_I16 != 0 {
            return Err(anyhow!(
                "PCM len {} not multiple of {} (20ms stereo frame)",
                pcm.len(),
                OPUS_FRAME_I16
            ));
        }

        let mut opus_buf = [0u8; 4000];
        let mut frames = Vec::new();

        for frame in pcm.chunks_exact(OPUS_FRAME_I16) {
            let n = self.enc.encode(frame, &mut opus_buf)?;
            frames.push(EncodedFrame {
                payload: opus_buf[..n].to_vec(),
                info: self.info.clone(),
            });
        }

        Ok(frames)
    }
}

fn opus_head() -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(b"OpusHead");
    p.push(1); // version
    p.push(2); // channels
    p.extend_from_slice(&312u16.to_le_bytes()); // pre-skip (typisch 312 @48k)
    p.extend_from_slice(&48_000u32.to_le_bytes());
    p.extend_from_slice(&0i16.to_le_bytes()); // gain
    p.push(0); // mapping family 0 (stereo)
    p
}

fn opus_tags(vendor: &str) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(b"OpusTags");
    p.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    p.extend_from_slice(vendor.as_bytes());
    p.extend_from_slice(&0u32.to_le_bytes()); // no comments
    p
}
