use crate::ring::{RingReader, RingRead};
use std::io::{self, Write};
use std::time::Duration;

pub fn run_pcm_pipe_out(mut r: RingReader) -> anyhow::Result<()> {
    let mut out = io::stdout().lock();

    loop {
        match r.poll() {
            RingRead::Chunk(slot) => {
                for s in slot.pcm.iter() {
                    if let Err(e) = out.write_all(&s.to_le_bytes()) {
                        if e.kind() == io::ErrorKind::BrokenPipe {
                            eprintln!("[pcm_pipe] downstream closed pipe");
                            return Ok(()); // sauber beenden
                        }
                        return Err(e.into());
                    }
                }
                if let Err(e) = out.flush() {
                    if e.kind() == io::ErrorKind::BrokenPipe {
                        eprintln!("[pcm_pipe] downstream closed pipe");
                        return Ok(());
                    }
                    return Err(e.into());
                }
            }
            RingRead::Gap { missed } => {
                eprintln!("[pcm_pipe] GAP missed={}", missed);
            }
            RingRead::Empty => {
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    }
}
