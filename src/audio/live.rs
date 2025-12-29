use std::io::Write;

use crate::audio::{EncodedFrameSource, EncodedRead};

pub fn stream_live(mut reader: impl EncodedFrameSource) -> anyhow::Result<()> {
    let mut stdout = std::io::stdout();

    loop {
        match reader.wait_for_read()? {
            EncodedRead::Frame(frame) => {
                stdout.write_all(&frame.payload)?;
            }
            EncodedRead::Gap { missed } => {
                eprintln!("[audio] live gap missed={}", missed);
            }
            EncodedRead::Empty => {}
        }
    }
}
