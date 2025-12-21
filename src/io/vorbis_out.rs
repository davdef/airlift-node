// src/io/vorbis_out.rs
use crate::ring::{RingRead, RingReader};
use anyhow::{anyhow, Result};
use log::info;
use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};
use crate::codecs::vorbis::VorbisEncoder;

pub struct VorbisConfig {
    pub host: String,
    pub port: u16,
    pub mount: String,
    pub user: String,
    pub password: String,
    pub name: String,
    pub description: String,
    pub genre: String,
    pub public: bool,
    pub quality: f32, // 0.0 to 1.0, z.B. 0.5 für ~128kbps
}


pub fn run_vorbis_out(mut r: RingReader, cfg: VorbisConfig) -> Result<()> {
    println!("[vorbis] Starting Vorbis stream to {}:{}{} (quality: {})",
             cfg.host, cfg.port, cfg.mount, cfg.quality);
    
    let mut backoff = Duration::from_secs(1);
    
    loop {
        match connect_and_stream_vorbis(&mut r, &cfg) {
            Ok(_) => backoff = Duration::from_secs(1),
            Err(e) => {
                eprintln!("[vorbis] Error: {} - reconnecting in {}s", 
                         e, backoff.as_secs());
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(Duration::from_secs(30));
            }
        }
    }
}

fn connect_and_stream_vorbis(r: &mut RingReader, cfg: &VorbisConfig) -> Result<()> {
    let mut stream = connect_vorbis(cfg)?;
    send_vorbis_headers(&mut stream, cfg)?;
    
    info!("[vorbis] Connected to {}:{}{}", cfg.host, cfg.port, cfg.mount);
    
    let mut encoder = VorbisEncoder::new(cfg.quality)?;
    let mut packets = 0;
    let mut last_log = Instant::now();
    
    loop {
        match r.poll() {
            RingRead::Chunk(slot) => {
                let data = encoder.encode_100ms(&slot.pcm)?;
                stream.write_all(&data)?;
                packets += 1;
                
                if packets == 1 {
                    println!("[vorbis] First packet sent ({} bytes)", data.len());
                }
            }
            RingRead::Gap { missed } => {
                eprintln!("[vorbis] Gap: missed {} chunks", missed);
            }
            RingRead::Empty => {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        
        if last_log.elapsed() >= Duration::from_secs(5) {
            let fill = r.fill();
            println!("[vorbis] Status: packets={}, fill={}", packets, fill);
            last_log = Instant::now();
        }
    }
}

fn connect_vorbis(cfg: &VorbisConfig) -> Result<TcpStream> {
    let addr = (cfg.host.as_str(), cfg.port)
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| anyhow!("DNS resolve failed"))?;
    
    let stream = std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(5))?;
    stream.set_nodelay(true)?;
    Ok(stream)
}

fn send_vorbis_headers(stream: &mut TcpStream, cfg: &VorbisConfig) -> Result<()> {
    use base64::{engine::general_purpose, Engine as _};
    
    let auth = format!("{}:{}", cfg.user, cfg.password);
    let auth = general_purpose::STANDARD.encode(auth);
    
    let public = if cfg.public { "1" } else { "0" };
    
    let hdr = format!(
        "SOURCE {} HTTP/1.0\r\n\
         Authorization: Basic {}\r\n\
         Content-Type: audio/ogg\r\n\
         Ice-Name: {}\r\n\
         Ice-Description: {}\r\n\
         Ice-Genre: {}\r\n\
         Ice-Public: {}\r\n\
         Ice-Audio-Info: samplerate=48000;channels=2\r\n\
         \r\n",
        cfg.mount,
        auth,
        cfg.name,
        cfg.description,
        cfg.genre,
        public
    );
    
    stream.write_all(hdr.as_bytes())?;
    Ok(())
}
