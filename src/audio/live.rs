// src/audio/live.rs
use std::process::{Command, Stdio};
use crate::ring::{RingRead, RingReader};

pub fn stream_live(mut reader: RingReader) -> anyhow::Result<()> {
    let mut ffmpeg = Command::new("ffmpeg")
        .args([
            "-loglevel", "quiet",
            "-f", "s16le",
            "-ar", "48000",
            "-ac", "2",
            "-i", "pipe:0",
            "-f", "mp3",
            "pipe:1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;

    let mut stdin = ffmpeg.stdin.take().unwrap();

    loop {
        match reader.poll() {
            RingRead::Chunk(slot) => {
                let bytes = bytemuck::cast_slice::<i16, u8>(&slot.pcm);
                stdin.write_all(bytes)?;
            }
            _ => std::thread::sleep(std::time::Duration::from_millis(5)),
        }
    }
}
