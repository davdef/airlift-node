// src/io/icecast_out.rs

use crate::codecs::AudioCodec;
use crate::config::IcecastOutputConfig;
use crate::ring::{RingRead, RingReader};
use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose, Engine as _};
use log::{info, warn};
use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::thread;
use std::time::{Duration, Instant};

// ============================================================
// Public entry
// ============================================================

pub fn run_icecast_out(mut r: RingReader, cfg: IcecastOutputConfig) -> Result<()> {
    let mut backoff = Duration::from_secs(1);

    loop {
        match connect_and_stream(&mut r, &cfg) {
            Ok(_) => backoff = Duration::from_secs(1),
            Err(e) => {
                warn!("[icecast] error: {}", e);
                warn!("[icecast] reconnect in {}s", backoff.as_secs());
                thread::sleep(backoff);
                backoff = (backoff * 2).min(Duration::from_secs(30));
            }
        }
    }
}

// ============================================================
// Core loop
// ============================================================

fn connect_and_stream(r: &mut RingReader, cfg: &IcecastOutputConfig) -> Result<()> {
    let mut stream = connect(cfg)?;
    let mut codec = cfg.codec.build()?;
    let content_type = codec.content_type();
    send_headers(&mut stream, cfg, content_type)?;

    info!(
        "[icecast] connected → {}:{}{} ({})",
        cfg.host,
        cfg.port,
        cfg.mount,
        cfg.codec.label()
    );

    // --- Stats ---
    let mut gaps: u64 = 0;
    let mut missed_total: u64 = 0;
    let mut packets: u64 = 0;
    let mut last_log = Instant::now();

    loop {
        match r.poll() {
            RingRead::Chunk(slot) => {
                let encoded = codec.encode_100ms(&slot.pcm)?;
                if !encoded.is_empty() {
                    stream.write_all(&encoded)?;
                }
                packets += 1;
            }

            RingRead::Gap { missed } => {
                gaps += 1;
                missed_total += missed;
            }

            RingRead::Empty => {
                thread::sleep(Duration::from_millis(5));
            }
        }

        // --- Periodisches Logging ---
        if last_log.elapsed() >= Duration::from_secs(5) {
            // (dein ursprüngliches Fill-Logging war ring-internal; falls das wieder privat ist, kannst du
            // hier einfach r.fill() nutzen – ich lasse es wie in deinem Anhang, weil es bei dir so war.)
            let fill = r.fill();
            eprintln!(
                "[icecast] fill={} slots | packets={} | GAPs={} missed={} | codec={}",
                fill,
                packets,
                gaps,
                missed_total,
                cfg.codec.label()
            );

            gaps = 0;
            missed_total = 0;
            packets = 0;
            last_log = Instant::now();
        }
    }
}

// ============================================================
// Icecast / TCP
// ============================================================

fn connect(cfg: &IcecastOutputConfig) -> Result<TcpStream> {
    let addr = (cfg.host.as_str(), cfg.port)
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| anyhow!("DNS resolve failed"))?;

    let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5))
        .context("connect failed")?;

    stream.set_nodelay(true).ok();
    Ok(stream)
}

fn send_headers(stream: &mut TcpStream, cfg: &IcecastOutputConfig, content_type: &str) -> Result<()> {
    let auth = format!("{}:{}", cfg.user, cfg.password);
    let auth = general_purpose::STANDARD.encode(auth);

    let public = if cfg.public { "1" } else { "0" };

    let hdr = format!(
        "SOURCE {} HTTP/1.0\r\n\
         Authorization: Basic {}\r\n\
         Content-Type: {}\r\n\
         Ice-Name: {}\r\n\
         Ice-Description: {}\r\n\
         Ice-Genre: {}\r\n\
         Ice-Public: {}\r\n\
         Ice-Audio-Info: samplerate=48000;channels=2\r\n\
         \r\n",
        cfg.mount,
        auth,
        content_type,
        sanitize(&cfg.name),
        sanitize(&cfg.description),
        sanitize(&cfg.genre),
        public
    );

    stream.write_all(hdr.as_bytes())?;
    Ok(())
}

fn sanitize(s: &str) -> String {
    s.replace('\r', " ").replace('\n', " ")
}
