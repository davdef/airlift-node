use serde::Serialize;

#[derive(Clone, Debug)]
pub struct PcmFrame {
    pub utc_ns: u64,
    pub samples: Vec<i16>,
    pub sample_rate: u32,
    pub channels: u8,
}

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

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum ContainerKind {
    Raw,
    Ogg,
    Mpeg,
    Rtp,
}
