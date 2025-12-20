// src/io/mp3_out.rs
use crate::ring::{RingRead, RingReader};
use anyhow::{Result, anyhow};
#[cfg(feature = "mp3")]
use lame::Lame;
use log::info;
use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

pub struct Mp3Config {
    pub host: String,
    pub port: u16,
    pub mount: String,
    pub user: String,
    pub password: String,
    pub name: String,
    pub description: String,
    pub genre: String,
    pub public: bool,
    pub bitrate: u32, // z.B. 128 für 128kbps
}

struct Mp3Encoder {
    lame: Lame,
    buffer: Vec<u8>,
    left: Vec<i16>,
    right: Vec<i16>,
}

impl Mp3Encoder {
    fn new(bitrate: u32) -> Result<Self> {
        const FRAMES: usize = 4800;
        const MP3_BUFFER_SIZE: usize = FRAMES * 5 / 4 + 7200;
        let mut lame = Lame::new().ok_or_else(|| anyhow!("Failed to init LAME"))?;

        lame.set_sample_rate(48_000)
            .map_err(|e| anyhow!("lame: {:?}", e))?;
        lame.set_channels(2).map_err(|e| anyhow!("lame: {:?}", e))?;
        lame.set_kilobitrate(bitrate as i32)
            .map_err(|e| anyhow!("lame: {:?}", e))?;
        lame.set_quality(2).map_err(|e| anyhow!("lame: {:?}", e))?;
        lame.init_params().map_err(|e| anyhow!("lame: {:?}", e))?;

        Ok(Self {
            lame,
            buffer: vec![0u8; MP3_BUFFER_SIZE],
            left: vec![0i16; FRAMES],
            right: vec![0i16; FRAMES],
        })
    }

    fn encode_100ms(&mut self, pcm: &[i16]) -> Result<Vec<u8>> {
        const FRAMES: usize = 4800;

        if pcm.len() != FRAMES * 2 {
            return Err(anyhow!("Wrong PCM length for 100ms"));
        }

        for (i, pair) in pcm.chunks_exact(2).enumerate() {
            self.left[i] = pair[0];
            self.right[i] = pair[1];
        }

        let written = self
            .lame
            .encode(&self.left, &self.right, &mut self.buffer)
            .map_err(|e| anyhow!("lame encode: {:?}", e))?;

        Ok(self.buffer[..written].to_vec())
    }
}

pub fn run_mp3_out(mut r: RingReader, cfg: Mp3Config) -> Result<()> {
    println!(
        "[mp3] Starting MP3 stream to {}:{}{} ({} kbps)",
        cfg.host, cfg.port, cfg.mount, cfg.bitrate
    );

    let mut backoff = Duration::from_secs(1);

    loop {
        match connect_and_stream_mp3(&mut r, &cfg) {
            Ok(_) => backoff = Duration::from_secs(1),
            Err(e) => {
                eprintln!(
                    "[mp3] Error: {} - reconnecting in {}s",
                    e,
                    backoff.as_secs()
                );
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(Duration::from_secs(30));
            }
        }
    }
}

fn connect_and_stream_mp3(r: &mut RingReader, cfg: &Mp3Config) -> Result<()> {
    let mut stream = connect_mp3(cfg)?;
    send_mp3_headers(&mut stream, cfg)?;

    info!("[mp3] Connected to {}:{}{}", cfg.host, cfg.port, cfg.mount);

    let mut encoder = Mp3Encoder::new(cfg.bitrate)?;
    let mut packets = 0;
    let mut last_log = Instant::now();

    loop {
        match r.poll() {
            RingRead::Chunk(slot) => {
                let data = encoder.encode_100ms(&slot.pcm)?;
                stream.write_all(&data)?;
                packets += 1;

                if packets == 1 {
                    println!("[mp3] First packet sent ({} bytes)", data.len());
                }
            }
            RingRead::Gap { missed } => {
                eprintln!("[mp3] Gap: missed {} chunks", missed);
            }
            RingRead::Empty => {
                std::thread::sleep(Duration::from_millis(5));
            }
        }

        if last_log.elapsed() >= Duration::from_secs(5) {
            let fill = r.fill();
            println!("[mp3] Status: packets={}, fill={}", packets, fill);
            last_log = Instant::now();
        }
    }
}

fn connect_mp3(cfg: &Mp3Config) -> Result<TcpStream> {
    let addr = (cfg.host.as_str(), cfg.port)
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| anyhow!("DNS resolve failed"))?;

    let stream = std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(5))?;
    stream.set_nodelay(true)?;
    Ok(stream)
}

fn send_mp3_headers(stream: &mut TcpStream, cfg: &Mp3Config) -> Result<()> {
    use base64::{Engine as _, engine::general_purpose};

    let auth = format!("{}:{}", cfg.user, cfg.password);
    let auth = general_purpose::STANDARD.encode(auth);

    let public = if cfg.public { "1" } else { "0" };

    let hdr = format!(
        "SOURCE {} HTTP/1.0\r\n\
         Authorization: Basic {}\r\n\
         Content-Type: audio/mpeg\r\n\
         Ice-Name: {}\r\n\
         Ice-Description: {}\r\n\
         Ice-Genre: {}\r\n\
         Ice-Public: {}\r\n\
         icy-br: {}\r\n\
         \r\n",
        cfg.mount, auth, cfg.name, cfg.description, cfg.genre, public, cfg.bitrate
    );

    stream.write_all(hdr.as_bytes())?;
    Ok(())
}
