// src/io/mp3_out.rs
use crate::codecs::registry::CodecRegistry;
use crate::ring::{RingRead, RingReader};
use anyhow::{anyhow, Result};
use log::info;
use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
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
    pub bitrate: u32,
    pub codec_id: Option<String>,
}

pub fn run_mp3_out(
    mut r: RingReader,
    cfg: Mp3Config,
    codec_registry: Arc<CodecRegistry>,
) -> Result<()> {
    println!(
        "[mp3] Starting MP3 stream to {}:{}{} ({} kbps)",
        cfg.host, cfg.port, cfg.mount, cfg.bitrate
    );

    let mut backoff = Duration::from_secs(1);

    loop {
        match connect_and_stream_mp3(&mut r, &cfg, codec_registry.clone()) {
            Ok(_) => {
                backoff = Duration::from_secs(1);
                println!("[mp3] Connection closed normally, reconnecting...");
            }
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

fn connect_and_stream_mp3(
    r: &mut RingReader,
    cfg: &Mp3Config,
    codec_registry: Arc<CodecRegistry>,
) -> Result<()> {
    let mut stream = connect_mp3(cfg)?;
    send_mp3_headers(&mut stream, cfg)?;

    info!("[mp3] Connected to {}:{}{}", cfg.host, cfg.port, cfg.mount);

    let codec_id = require_codec_id(cfg.codec_id.as_deref())?;
    let (mut codec, codec_instance) = codec_registry.build_codec(codec_id)?;
    codec_instance.mark_ready();
    let mut packets = 0;
    let mut last_log = Instant::now();

    loop {
        match r.poll() {
            RingRead::Chunk(slot) => {
                let frames = codec.encode(&slot.pcm).map_err(|e| {
                    codec_instance.mark_error(&e.to_string());
                    e
                })?;
                let mut bytes = 0u64;
                let frame_count = frames.len() as u64;
                for frame in frames {
                    // Sicherstellen, dass wir Daten haben
                    if frame.payload.is_empty() {
                        continue;
                    }
                    bytes += frame.payload.len() as u64;
                    if let Err(e) = stream.write_all(&frame.payload) {
                        return Err(anyhow!("Failed to write MP3 data: {}", e));
                    }

                    if let Err(e) = stream.flush() {
                        return Err(anyhow!("Failed to flush stream: {}", e));
                    }

                    packets += 1;

                    if packets == 1 {
                        println!("[mp3] First packet sent ({} bytes)", frame.payload.len());
                    }
                }
                codec_instance.mark_encoded(1, frame_count, bytes);
            }
            RingRead::Gap { missed } => {
                eprintln!("[mp3] Gap: missed {} chunks", missed);
                // Optional: Stille generieren für Lücken
            }
            RingRead::Empty => {
                std::thread::sleep(Duration::from_millis(5));
            }
        }

        if last_log.elapsed() >= Duration::from_secs(5) {
            let fill = r.fill();
            println!("[mp3] Status: packets={}, fill={}%", packets, fill);
            last_log = Instant::now();
        }
    }
}

fn connect_mp3(cfg: &Mp3Config) -> Result<TcpStream> {
    let addr = format!("{}:{}", cfg.host, cfg.port);
    let addrs: Vec<_> = addr.to_socket_addrs()?.collect();

    if addrs.is_empty() {
        return Err(anyhow!("No addresses found for {}:{}", cfg.host, cfg.port));
    }

    // Versuche alle aufgelösten Adressen
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, Duration::from_secs(5)) {
            Ok(stream) => {
                stream.set_nodelay(true)?;
                stream.set_write_timeout(Some(Duration::from_secs(10)))?;
                return Ok(stream);
            }
            Err(e) => {
                eprintln!("[mp3] Failed to connect to {}: {}", addr, e);
                continue;
            }
        }
    }

    Err(anyhow!(
        "Failed to connect to any address for {}:{}",
        cfg.host,
        cfg.port
    ))
}

fn send_mp3_headers(stream: &mut TcpStream, cfg: &Mp3Config) -> Result<()> {
    use base64::{engine::general_purpose, Engine as _};

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
    stream.flush()?;

    // Kurze Pause für Server-Antwort (optional)
    std::thread::sleep(Duration::from_millis(100));

    Ok(())
}

fn require_codec_id(codec_id: Option<&str>) -> Result<&str> {
    codec_id.ok_or_else(|| anyhow!("missing codec_id for mp3 output"))
}
