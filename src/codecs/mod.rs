use anyhow::Result;
use serde::Deserialize;

pub mod opus;
pub mod vorbis;
#[cfg(feature = "mp3")]
pub mod mp3;

pub trait AudioCodec: Send {
    fn content_type(&self) -> &'static str;
    fn encode_100ms(&mut self, pcm: &[i16]) -> Result<Vec<u8>>;
}

#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum CodecConfig {
    Opus {
        bitrate: i32,
        #[serde(default = "default_opus_vendor")]
        vendor: String,
    },
    #[cfg(feature = "mp3")]
    Mp3 { bitrate: u32 },
    Vorbis { quality: f32 },
}

impl CodecConfig {
    pub fn build(&self) -> Result<Box<dyn AudioCodec>> {
        match self {
            CodecConfig::Opus { bitrate, vendor } => {
                Ok(Box::new(opus::OggOpusEncoder::new(*bitrate, vendor)?))
            }
            #[cfg(feature = "mp3")]
            CodecConfig::Mp3 { bitrate } => Ok(Box::new(mp3::Mp3Encoder::new(*bitrate, 48_000)?)),
            CodecConfig::Vorbis { quality } => Ok(Box::new(vorbis::VorbisEncoder::new(*quality)?)),
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            CodecConfig::Opus { .. } => "opus",
            #[cfg(feature = "mp3")]
            CodecConfig::Mp3 { .. } => "mp3",
            CodecConfig::Vorbis { .. } => "vorbis",
        }
    }
}

fn default_opus_vendor() -> String {
    "airlift".to_string()
}
