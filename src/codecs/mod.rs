use serde::Serialize;

pub mod opus;
pub mod pcm;
pub mod registry;
pub mod vorbis;
#[cfg(feature = "mp3")]
pub mod mp3;

pub const PCM_SAMPLE_RATE: u32 = 48_000;
pub const PCM_CHANNELS: u8 = 2;
pub const PCM_FRAME_MS: u32 = 100;
pub const PCM_SAMPLES_PER_CH: usize = (PCM_SAMPLE_RATE as usize / 1000) * PCM_FRAME_MS as usize;
pub const PCM_I16_SAMPLES: usize = PCM_SAMPLES_PER_CH * PCM_CHANNELS as usize;

#[derive(Clone, Debug, Serialize)]
pub struct CodecInfo {
    pub kind: CodecKind,
    pub sample_rate: u32,
    pub channels: u8,
    pub container: ContainerKind,
}

#[derive(Clone, Debug, Serialize)]
pub struct EncodedFrame {
    pub payload: Vec<u8>,
    pub info: CodecInfo,
}

#[derive(Clone, Debug, Serialize)]
pub enum CodecKind {
    Pcm,
    OpusOgg,
    OpusWebRtc,
    Mp3,
    Vorbis,
    AacLc,
    Flac,
}

#[derive(Clone, Debug, Serialize)]
pub enum ContainerKind {
    Raw,
    Ogg,
    Mpeg,
    Rtp,
}

pub trait AudioCodec {
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
