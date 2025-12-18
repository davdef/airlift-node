// src/io/icecast_out.rs

use crate::ring::{RingRead, RingReader};
use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose, Engine as _};
use log::{info, warn};
use ogg::writing::PacketWriter;
use opus::{Application, Channels, Encoder};
use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::thread;
use std::time::{Duration, Instant};

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

pub fn run_icecast_out(mut r: RingReader, cfg: IcecastConfig) -> Result<()> {
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

fn connect_and_stream(r: &mut RingReader, cfg: &IcecastConfig) -> Result<()> {
    let mut stream = connect(cfg)?;
    send_headers(&mut stream, cfg)?;

    info!(
        "[icecast] connected → {}:{}{}",
        cfg.host, cfg.port, cfg.mount
    );

    let mut ogg = OggOpus::new(cfg.opus_bitrate)?;

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

struct OggOpus {
    enc: Encoder,
    pw: PacketWriter<Vec<u8>>,
    serial: u32,
    gp: u64, // granule position (48k samples)
}

impl OggOpus {
    fn new(bitrate: i32) -> Result<Self> {
        let mut enc = Encoder::new(48_000, Channels::Stereo, Application::Audio)?;
        enc.set_bitrate(opus::Bitrate::Bits(bitrate))?;

        let serial = rand::random::<u32>();
        let mut pw = PacketWriter::new(Vec::with_capacity(64 * 1024));

        // Wichtig: Head/Tags genau einmal am Anfang (BOS)
        pw.write_packet(
            opus_head().into(),
            serial,
            ogg::PacketWriteEndInfo::EndPage,
            0,
        )?;
        pw.write_packet(
            opus_tags("airlift").into(),
            serial,
            ogg::PacketWriteEndInfo::EndPage,
            0,
        )?;

        Ok(Self {
            enc,
            pw,
            serial,
            gp: 0,
        })
    }

    fn encode_100ms(&mut self, pcm: &[i16]) -> Result<Vec<u8>> {
        // 100 ms @ 48 kHz stereo = 4800 Frames = 9600 i16 samples
        // Wir encoden in 20ms-Frames: 960 Frames pro Kanal => 960*2 i16 pro Frame
        const OPUS_FRAME_SAMPLES_PER_CH: usize = 960; // 20 ms @ 48k
        const CHANNELS: usize = 2;
        const OPUS_FRAME_I16: usize = OPUS_FRAME_SAMPLES_PER_CH * CHANNELS;

        if pcm.len() % OPUS_FRAME_I16 != 0 {
            return Err(anyhow!(
                "PCM len {} not multiple of {} (20ms stereo frame)",
                pcm.len(),
                OPUS_FRAME_I16
            ));
        }

        let mut opus_buf = [0u8; 4000];

        let mut frames = pcm.chunks_exact(OPUS_FRAME_I16);
        let n_frames = frames.len();
        if n_frames == 0 {
            return Ok(Vec::new());
        }
        let last = n_frames - 1;

        for (i, frame) in frames.by_ref().enumerate() {
            let n = self.enc.encode(frame, &mut opus_buf)?;

            // Granule = Ende dieses 20ms-Pakets (in 48k-Samples pro Kanal)
            self.gp += OPUS_FRAME_SAMPLES_PER_CH as u64;

            let end = if i == last {
                ogg::PacketWriteEndInfo::EndPage
            } else {
                ogg::PacketWriteEndInfo::NormalPacket
            };

            self.pw.write_packet(
                opus_buf[..n].to_vec().into_boxed_slice(),
                self.serial,
                end,
                self.gp,
            )?;
        }

        // KRITISCH: Writer NICHT ersetzen (sonst wieder BOS => Icecast meckert)
        // Stattdessen nur den inneren Vec leeren und rausholen:
        let mut out = Vec::new();
        std::mem::swap(&mut out, self.pw.inner_mut());
        Ok(out)
    }
}

// ============================================================
// Ogg headers
// ============================================================

fn opus_head() -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(b"OpusHead");
    p.push(1); // version
    p.push(2); // channels
    p.extend_from_slice(&312u16.to_le_bytes()); // pre-skip (typisch 312 @48k)
    p.extend_from_slice(&48_000u32.to_le_bytes());
    p.extend_from_slice(&0i16.to_le_bytes()); // gain
    p.push(0); // mapping family 0 (stereo)
    p
}

fn opus_tags(vendor: &str) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(b"OpusTags");
    p.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    p.extend_from_slice(vendor.as_bytes());
    p.extend_from_slice(&0u32.to_le_bytes()); // no comments
    p
}

