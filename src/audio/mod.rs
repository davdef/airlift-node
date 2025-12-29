use crate::codecs::EncodedFrame;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::ring::{EncodedRingRead, EncodedSource};

pub mod http;
pub mod live;
pub mod path;
pub mod timeshift;

pub use path::sanitize_audio_path;

pub enum EncodedRead {
    Frame(EncodedFrame),
    Gap { missed: u64 },
    Empty,
}

pub trait EncodedFrameSource: Send {
    fn poll(&mut self) -> anyhow::Result<EncodedRead>;
    fn wait_for_read(&mut self) -> anyhow::Result<EncodedRead>;
    fn wait_for_read_or_stop(&mut self, stop: &AtomicBool) -> anyhow::Result<Option<EncodedRead>>;
    fn notifier(&self) -> Option<Arc<std::sync::Condvar>> {
        None
    }
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

    fn wait_for_read(&mut self) -> anyhow::Result<EncodedRead> {
        let read = EncodedSource::wait_for_read(self);
        Ok(match read {
            EncodedRingRead::Frame { frame, .. } => EncodedRead::Frame(frame),
            EncodedRingRead::Gap { missed } => EncodedRead::Gap { missed },
            EncodedRingRead::Empty => EncodedRead::Empty,
        })
    }

    fn wait_for_read_or_stop(&mut self, stop: &AtomicBool) -> anyhow::Result<Option<EncodedRead>> {
        Ok(
            EncodedSource::wait_for_read_or_stop(self, stop).map(|read| match read {
                EncodedRingRead::Frame { frame, .. } => EncodedRead::Frame(frame),
                EncodedRingRead::Gap { missed } => EncodedRead::Gap { missed },
                EncodedRingRead::Empty => EncodedRead::Empty,
            }),
        )
    }

    fn notifier(&self) -> Option<Arc<std::sync::Condvar>> {
        EncodedSource::notifier(self)
    }
}
