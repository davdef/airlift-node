// src/io/icecast_out.rs

use crate::ring::{RingRead, RingReader};
use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose, Engine as _};
use log::{info, warn};
use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::codecs::opus::OggOpusEncoder;
use crate::control::ModuleState;
// ============================================================
// Config
// ============================================================

pub struct IcecastConfig {
    pub host: String,
    pub port: u16,
    pub mount: String, // "/rfm.ogg"

    pub user: String, // usually "source"
    pub password: String,

    pub name: String,
    pub description: String,
    pub genre: String,
    pub public: bool,

    pub opus_bitrate: i32, // z.B. 64000 .. 128000
}

// ============================================================
// Public entry
// ============================================================

pub fn run_icecast_out(
    mut r: RingReader,
    cfg: IcecastConfig,
    state: Arc<ModuleState>,
    ring_state: Arc<ModuleState>,
) -> Result<()> {
    state.set_running(true);
    let mut backoff = Duration::from_secs(1);

    loop {
        match connect_and_stream(&mut r, &cfg, state.clone(), ring_state.clone()) {
            Ok(_) => {
                state.set_connected(false);
                backoff = Duration::from_secs(1)
            }
            Err(e) => {
                warn!("[icecast] error: {}", e);
                state.mark_error(1);
                state.set_connected(false);
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

fn connect_and_stream(
    r: &mut RingReader,
    cfg: &IcecastConfig,
    state: Arc<ModuleState>,
    ring_state: Arc<ModuleState>,
) -> Result<()> {
    let mut stream = connect(cfg)?;
    send_headers(&mut stream, cfg)?;

    info!(
        "[icecast] connected → {}:{}{}",
        cfg.host, cfg.port, cfg.mount
    );
    state.set_connected(true);

    let mut ogg = OggOpusEncoder::new(cfg.opus_bitrate, "airlift")?;

    // --- Stats ---
    let mut gaps: u64 = 0;
    let mut missed_total: u64 = 0;
    let mut packets: u64 = 0;
    let mut last_log = Instant::now();

    loop {
        match r.poll() {
            RingRead::Chunk(slot) => {
                let ogg_bytes = ogg.encode_100ms(&slot.pcm)?;
                stream.write_all(&ogg_bytes)?;
                packets += 1;
                state.mark_tx(1);
                ring_state.mark_tx(1);
            }

            RingRead::Gap { missed } => {
                gaps += 1;
                missed_total += missed;
                state.mark_drop(missed);
                ring_state.mark_drop(missed);
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
                "[icecast] fill={} slots | packets={} | GAPs={} missed={}",
                fill, packets, gaps, missed_total
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

fn connect(cfg: &IcecastConfig) -> Result<TcpStream> {
    let addr = (cfg.host.as_str(), cfg.port)
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| anyhow!("DNS resolve failed"))?;

    let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5))
        .context("connect failed")?;

    stream.set_nodelay(true).ok();
    Ok(stream)
}

fn send_headers(stream: &mut TcpStream, cfg: &IcecastConfig) -> Result<()> {
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

// ============================================================
// Opus / Ogg encoder
// ============================================================
