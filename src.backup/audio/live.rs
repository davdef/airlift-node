// src/audio/live.rs
use std::io::Write;

use crate::codecs::EncodedFrame;

pub enum EncodedRead {
    Frame(EncodedFrame),
    Gap { missed: u64 },
    Empty,
}

pub trait EncodedFrameSource: Send {
    fn poll(&mut self) -> anyhow::Result<EncodedRead>;
}

pub fn stream_live(mut reader: impl EncodedFrameSource) -> anyhow::Result<()> {
    let mut stdout = std::io::stdout();

    loop {
        match reader.poll()? {
            EncodedRead::Frame(frame) => {
                stdout.write_all(&frame.payload)?;
            }
            EncodedRead::Gap { missed } => {
                eprintln!("[audio] live gap missed={}", missed);
            }
            EncodedRead::Empty => std::thread::sleep(std::time::Duration::from_millis(5)),
        }
    }
}

impl EncodedFrameSource for crate::ring::RingReader {
    fn poll(&mut self) -> anyhow::Result<EncodedRead> {
        Err(anyhow::anyhow!(
            "PCM ring reader not supported for encoded outputs"
        ))
    }
}
