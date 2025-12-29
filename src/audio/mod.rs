use crate::codecs::EncodedFrame;
use crate::ring::{EncodedRingRead, EncodedSource};

pub mod http;
pub mod live;
pub mod timeshift;

pub enum EncodedRead {
    Frame(EncodedFrame),
    Gap { missed: u64 },
    Empty,
}

pub trait EncodedFrameSource: Send {
    fn poll(&mut self) -> anyhow::Result<EncodedRead>;
}

impl<T> EncodedFrameSource for T
where
    T: EncodedSource + Send,
{
    fn poll(&mut self) -> anyhow::Result<EncodedRead> {
        let read = EncodedSource::poll(self);
        Ok(match read {
            EncodedRingRead::Frame { frame, .. } => EncodedRead::Frame(frame),
            EncodedRingRead::Gap { missed } => EncodedRead::Gap { missed },
            EncodedRingRead::Empty => EncodedRead::Empty,
        })
    }
}
