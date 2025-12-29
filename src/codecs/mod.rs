pub mod pcm;

pub use crate::types::{CodecInfo, CodecKind, ContainerKind, EncodedFrame};

pub const PCM_SAMPLE_RATE: u32 = 48_000;
pub const PCM_CHANNELS: u8 = 2;
pub const PCM_FRAME_MS: u32 = 100;
pub const PCM_SAMPLES_PER_CH: usize = (PCM_SAMPLE_RATE as usize / 1000) * PCM_FRAME_MS as usize;
pub const PCM_I16_SAMPLES: usize = PCM_SAMPLES_PER_CH * PCM_CHANNELS as usize;

pub trait AudioCodec: Send {
    fn info(&self) -> &CodecInfo;
    fn encode(&mut self, pcm: &[i16]) -> anyhow::Result<Vec<EncodedFrame>>;
}

pub fn supported_codecs() -> Vec<CodecInfo> {
    let mut codecs = vec![
        CodecInfo {
            kind: CodecKind::Pcm,
            sample_rate: PCM_SAMPLE_RATE,
            channels: PCM_CHANNELS,
            container: ContainerKind::Raw,
        },
        CodecInfo {
            kind: CodecKind::OpusOgg,
            sample_rate: PCM_SAMPLE_RATE,
            channels: PCM_CHANNELS,
            container: ContainerKind::Ogg,
        },
        CodecInfo {
            kind: CodecKind::OpusWebRtc,
            sample_rate: PCM_SAMPLE_RATE,
            channels: PCM_CHANNELS,
            container: ContainerKind::Rtp,
        },
        CodecInfo {
            kind: CodecKind::Vorbis,
            sample_rate: PCM_SAMPLE_RATE,
            channels: PCM_CHANNELS,
            container: ContainerKind::Ogg,
        },
        CodecInfo {
            kind: CodecKind::AacLc,
            sample_rate: PCM_SAMPLE_RATE,
            channels: PCM_CHANNELS,
            container: ContainerKind::Raw,
        },
        CodecInfo {
            kind: CodecKind::Flac,
            sample_rate: PCM_SAMPLE_RATE,
            channels: PCM_CHANNELS,
            container: ContainerKind::Raw,
        },
    ];

    #[cfg(feature = "mp3")]
    codecs.push(CodecInfo {
        kind: CodecKind::Mp3,
        sample_rate: PCM_SAMPLE_RATE,
        channels: PCM_CHANNELS,
        container: ContainerKind::Mpeg,
    });

    codecs
}
