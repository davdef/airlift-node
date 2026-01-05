use std::io::{Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use crossbeam_channel::{unbounded, Receiver, Sender};
use tiny_http::{Header, ReadWrite, Request, Response, StatusCode};

use crate::api::recorder::get_echo_sender; // This should now exist
use crate::core::lock::lock_mutex;
use crate::core::{timestamp, AirliftNode, Event, EventHandler, EventPriority, EventType, PcmFrame};
use crate::producers::ws::WsHandle;

const WEBSOCKET_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
static WS_HANDLER_COUNTER: AtomicU64 = AtomicU64::new(1);
const RECORDER_SAMPLE_RATE: u32 = 48_000;

pub fn handle_ws_request(request: Request, node: Arc<Mutex<AirliftNode>>) {
    thread::spawn(move || {
        if !is_websocket_request(&request) {
            let _ = request.respond(Response::empty(StatusCode(400)));
            return;
        }

        let key = match websocket_key(&request) {
            Some(key) => key,
            None => {
                let _ = request.respond(Response::empty(StatusCode(400)));
                return;
            }
        };

        let accept = websocket_accept_key(&key);
        let response = Response::empty(StatusCode(101))
            .with_header(make_header("Upgrade", "websocket"))
            .with_header(make_header("Connection", "Upgrade"))
            .with_header(make_header("Sec-WebSocket-Accept", &accept));

        let mut stream = request.upgrade("websocket", response);
        let event_bus = {
            let node = lock_mutex(&node, "api.ws.event_bus");
            node.event_bus()
        };

        let (sender, receiver) = unbounded();
        let handler_name = format!(
            "ws-audio-{}",
            WS_HANDLER_COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let handler = Arc::new(WsEventHandler::new(handler_name.clone(), sender));

        {
            let bus = lock_mutex(&event_bus, "api.ws.register_handler");
            if let Err(error) = bus.register_handler(handler) {
                log::error!(
                    "Failed to register websocket handler '{}': {}",
                    handler_name,
                    error
                );
                return;
            }
        }

        if let Err(error) = stream_audio_peaks(&mut stream, receiver) {
            log::info!(
                "Websocket stream '{}' closed: {}",
                handler_name,
                error
            );
        }

        let bus = lock_mutex(&event_bus, "api.ws.unregister_handler");
        let _ = bus.unregister_handler(&handler_name);
    });
}

pub fn handle_recorder_ws_request(
    request: Request,
    _node: Arc<Mutex<AirliftNode>>,
    producer_id: String,
) {
    thread::spawn(move || {
        let Some(handle) = crate::api::recorder::get_recorder_handle(&producer_id) else {
            let _ = request.respond(Response::empty(StatusCode(404)));
            return;
        };

        if !is_websocket_request(&request) {
            let _ = request.respond(Response::empty(StatusCode(400)));
            return;
        }

        let key = match websocket_key(&request) {
            Some(key) => key,
            None => {
                let _ = request.respond(Response::empty(StatusCode(400)));
                return;
            }
        };

        let accept = websocket_accept_key(&key);
        let response = Response::empty(StatusCode(101))
            .with_header(make_header("Upgrade", "websocket"))
            .with_header(make_header("Connection", "Upgrade"))
            .with_header(make_header("Sec-WebSocket-Accept", &accept));

        let mut stream = request.upgrade("websocket", response);
        if let Err(error) = read_recorder_frames(&mut stream, &handle, &producer_id) {
            log::info!(
                "Recorder websocket '{}' closed: {}",
                producer_id,
                error
            );
        }
    });
}

pub fn handle_echo_ws_request(
    request: Request,
    _node: Arc<Mutex<AirliftNode>>,
    session_id: String,
) {
    thread::spawn(move || {
        log::info!("Echo WebSocket requested for session: {}", session_id);
        
        // Hole den Echo-Sender für diese Session
        let Some(echo_sender) = get_echo_sender(&session_id) else {
            log::warn!("No echo sender found for session: {}", session_id);
            let _ = request.respond(Response::empty(StatusCode(404)));
            return;
        };

        log::info!("Echo sender found for session: {}", session_id);

        // WebSocket-Handshake
        if !is_websocket_request(&request) {
            let _ = request.respond(Response::empty(StatusCode(400)));
            return;
        }

        let key = match websocket_key(&request) {
            Some(key) => key,
            None => {
                let _ = request.respond(Response::empty(StatusCode(400)));
                return;
            }
        };

        let accept = websocket_accept_key(&key);
        let response = Response::empty(StatusCode(101))
            .with_header(make_header("Upgrade", "websocket"))
            .with_header(make_header("Connection", "Upgrade"))
            .with_header(make_header("Sec-WebSocket-Accept", &accept));

        let mut stream = request.upgrade("websocket", response);
        
        log::info!("Echo WebSocket connected for session: {}", session_id);
        
        // Erstelle einen Channel für diesen Client
        let (client_sender, client_receiver) = unbounded::<PcmFrame>();
        
        // Starte einen Thread, der Frames vom Client zum Echo-Sender forwardet
        let echo_sender_clone = echo_sender.clone();
        let session_id_clone = session_id.clone();
        thread::spawn(move || {
            log::info!("Starting echo client handler for session: {}", session_id_clone);
            
            for frame in client_receiver.iter() {
                if echo_sender_clone.send(frame).is_err() {
                    log::info!("Echo client handler '{}': session closed", session_id_clone);
                    break;
                }
            }
            
            log::info!("Echo client handler stopped for session: {}", session_id_clone);
        });
        
        // Fix the type inference issue
        let _sender = client_sender;
        
        // Sende Audio-Frames an Client
        if let Err(error) = stream_echo_frames(&mut stream, &session_id) {
            log::info!("Echo websocket '{}' closed: {}", session_id, error);
        }
    });
}

fn stream_echo_frames(
    stream: &mut dyn ReadWrite,
    session_id: &str,
) -> std::io::Result<()> {
    log::info!("Starting echo stream for session: {}", session_id);
    
    // Diese Funktion wartet auf Close-Frame
    loop {
        let frame = read_ws_frame(stream)?;
        
        match frame.opcode {
            0x8 => { // Close
                log::info!("Echo WebSocket closed by client: {}", session_id);
                return Ok(());
            }
            0x9 => { // Ping
                write_ws_frame(stream, 0xA, &frame.payload)?; // Pong
            }
            _ => {
                // Ignoriere andere Frames
            }
        }
    }
}

fn is_websocket_request(request: &Request) -> bool {
    request
        .headers()
        .iter()
        .find(|h| h.field.equiv(&"Upgrade"))
        .map(|h| h.value.as_str().eq_ignore_ascii_case("websocket"))
        .unwrap_or(false)
}

fn websocket_key(request: &Request) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|h| h.field.equiv(&"Sec-WebSocket-Key"))
        .map(|h| h.value.as_str().to_string())
}

fn read_recorder_frames(
    stream: &mut dyn ReadWrite,
    handle: &WsHandle,
    producer_id: &str,
) -> std::io::Result<()> {
    loop {
        let frame = read_ws_frame(stream)?;
        match frame.opcode {
            0x2 => {
                if !frame.fin {
                    log::warn!(
                        "Recorder websocket '{}' received fragmented frame",
                        producer_id
                    );
                    return Ok(());
                }

                if frame.payload.len() % 4 != 0 {
                    log::warn!(
                        "Recorder websocket '{}' received invalid payload length {}",
                        producer_id,
                        frame.payload.len()
                    );
                    continue;
                }

                let mut samples = Vec::with_capacity(frame.payload.len() / 4);
                for chunk in frame.payload.chunks_exact(4) {
                    let sample = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                    samples.push(normalize_sample(sample));
                }

                if samples.is_empty() {
                    continue;
                }

                let frame = PcmFrame {
                    utc_ns: timestamp::utc_ns_now(),
                    samples,
                    sample_rate: RECORDER_SAMPLE_RATE,
                    channels: 2,
                };

                if let Err(error) = handle.push_frame(frame) {
                    log::warn!(
                        "Recorder websocket '{}' failed to push frame: {}",
                        producer_id,
                        error
                    );
                    return Ok(());
                }
            }
            0x8 => return Ok(()),
            0x9 => {
                write_ws_frame(stream, 0xA, &frame.payload)?;
            }
            0x1 => {
                log::debug!(
                    "Recorder websocket '{}' ignoring text frame",
                    producer_id
                );
            }
            _ => {
                log::debug!(
                    "Recorder websocket '{}' ignoring opcode {}",
                    producer_id,
                    frame.opcode
                );
            }
        }
    }
}

fn normalize_sample(sample: f32) -> i16 {
    let clamped = sample.clamp(-1.0, 1.0);
    (clamped * i16::MAX as f32) as i16
}

fn stream_audio_peaks(
    stream: &mut dyn ReadWrite,
    receiver: Receiver<String>,
) -> std::io::Result<()> {
    for payload in receiver.iter() {
        write_text_frame(stream, payload.as_bytes())?;
    }
    Ok(())
}

fn write_text_frame(stream: &mut dyn ReadWrite, payload: &[u8]) -> std::io::Result<()> {
    write_ws_frame(stream, 0x1, payload)
}

fn make_header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).unwrap()
}

fn write_ws_frame(
    stream: &mut dyn ReadWrite,
    opcode: u8,
    payload: &[u8],
) -> std::io::Result<()> {
    let mut header = Vec::with_capacity(10);
    header.push(0x80 | (opcode & 0x0F));

    match payload.len() {
        0..=125 => header.push(payload.len() as u8),
        126..=65535 => {
            header.push(126);
            header.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        }
        _ => {
            header.push(127);
            header.extend_from_slice(&(payload.len() as u64).to_be_bytes());
        }
    }

    stream.write_all(&header)?;
    stream.write_all(payload)?;
    stream.flush()
}

struct WsFrame {
    fin: bool,
    opcode: u8,
    payload: Vec<u8>,
}

fn read_ws_frame(stream: &mut dyn ReadWrite) -> std::io::Result<WsFrame> {
    let mut header = [0u8; 2];
    stream.read_exact(&mut header)?;

    let fin = (header[0] & 0x80) != 0;
    let opcode = header[0] & 0x0F;
    let masked = (header[1] & 0x80) != 0;
    let mut payload_len = (header[1] & 0x7F) as u64;

    if payload_len == 126 {
        let mut len_bytes = [0u8; 2];
        stream.read_exact(&mut len_bytes)?;
        payload_len = u16::from_be_bytes(len_bytes) as u64;
    } else if payload_len == 127 {
        let mut len_bytes = [0u8; 8];
        stream.read_exact(&mut len_bytes)?;
        payload_len = u64::from_be_bytes(len_bytes);
    }

    let payload_len: usize = match payload_len.try_into() {
        Ok(len) => len,
        Err(_) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "payload too large",
            ))
        }
    };

    let mut mask_key = [0u8; 4];
    if masked {
        stream.read_exact(&mut mask_key)?;
    }

    let mut payload = vec![0u8; payload_len];
    if payload_len > 0 {
        stream.read_exact(&mut payload)?;
    }

    if masked {
        for (i, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask_key[i % 4];
        }
    }

    Ok(WsFrame {
        fin,
        opcode,
        payload,
    })
}

fn websocket_accept_key(key: &str) -> String {
    let mut data = Vec::with_capacity(key.len() + WEBSOCKET_GUID.len());
    data.extend_from_slice(key.as_bytes());
    data.extend_from_slice(WEBSOCKET_GUID.as_bytes());

    let digest = Sha1::digest(&data);
    base64_encode(&digest)
}

struct WsEventHandler {
    name: String,
    sender: Sender<String>,
}

impl WsEventHandler {
    fn new(name: String, sender: Sender<String>) -> Self {
        Self { name, sender }
    }
}

impl EventHandler for WsEventHandler {
    fn handle_event(&self, event: &Event) -> anyhow::Result<()> {
        let payload = serde_json::to_string(&event.payload)?;
        let _ = self.sender.send(payload);
        Ok(())
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn priority_filter(&self) -> Option<EventPriority> {
        Some(EventPriority::Debug)
    }

    fn event_type_filter(&self) -> Option<Vec<EventType>> {
        Some(vec![EventType::AudioPeak])
    }
}

/* ===================== SHA1 ===================== */

struct Sha1 {
    state: [u32; 5],
    buffer: [u8; 64],
    buffer_len: usize,
    message_len: u64,
}

impl Sha1 {
    fn new() -> Self {
        Self {
            state: [
                0x67452301,
                0xEFCDAB89,
                0x98BADCFE,
                0x10325476,
                0xC3D2E1F0,
            ],
            buffer: [0; 64],
            buffer_len: 0,
            message_len: 0,
        }
    }

    fn digest(data: &[u8]) -> [u8; 20] {
        let mut hasher = Self::new();
        hasher.update(data);
        hasher.finalize()
    }

    fn update(&mut self, data: &[u8]) {
        self.message_len = self.message_len.saturating_add(data.len() as u64);
        let mut input = data;

        if self.buffer_len > 0 {
            let remaining = 64 - self.buffer_len;
            if input.len() >= remaining {
                self.buffer[self.buffer_len..64].copy_from_slice(&input[..remaining]);

                let block = self.buffer;
                self.process_block(&block);

                self.buffer_len = 0;
                input = &input[remaining..];
            } else {
                self.buffer[self.buffer_len..self.buffer_len + input.len()]
                    .copy_from_slice(input);
                self.buffer_len += input.len();
                return;
            }
        }

        for chunk in input.chunks_exact(64) {
            self.process_block(chunk);
        }

        let rem = input.len() % 64;
        if rem > 0 {
            let start = input.len() - rem;
            self.buffer[..rem].copy_from_slice(&input[start..]);
            self.buffer_len = rem;
        }
    }

    fn finalize(mut self) -> [u8; 20] {
        let bit_len = self.message_len * 8;

        self.buffer[self.buffer_len] = 0x80;
        self.buffer_len += 1;

        if self.buffer_len > 56 {
            self.buffer[self.buffer_len..64].fill(0);
            let block = self.buffer;
            self.process_block(&block);
            self.buffer_len = 0;
        }

        self.buffer[self.buffer_len..56].fill(0);
        self.buffer[56..64].copy_from_slice(&bit_len.to_be_bytes());

        let block = self.buffer;
        self.process_block(&block);

        let mut out = [0u8; 20];
        for (i, chunk) in out.chunks_exact_mut(4).enumerate() {
            chunk.copy_from_slice(&self.state[i].to_be_bytes());
        }
        out
    }

    fn process_block(&mut self, block: &[u8]) {
        let mut w = [0u32; 80];
        for (i, c) in block.chunks_exact(4).take(16).enumerate() {
            w[i] = u32::from_be_bytes([c[0], c[1], c[2], c[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let mut a = self.state[0];
        let mut b = self.state[1];
        let mut c = self.state[2];
        let mut d = self.state[3];
        let mut e = self.state[4];

        for i in 0..80 {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };

            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(w[i]);

            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
    }
}

fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(((data.len() + 2) / 3) * 4);

    for chunk in data.chunks(3) {
        let b = match chunk.len() {
            1 => [chunk[0], 0, 0],
            2 => [chunk[0], chunk[1], 0],
            _ => [chunk[0], chunk[1], chunk[2]],
        };
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;

        out.push(TABLE[((n >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3F) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[((n >> 6) & 0x3F) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(n & 0x3F) as usize] as char
        } else {
            '='
        });
    }
    out
}
