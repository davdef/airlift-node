// src/io/icecast_meta_out.rs
use anyhow::Result;
use base64::{engine::general_purpose, Engine as _};
use log::{info, warn, error};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use urlencoding::encode;

pub struct IcecastMetadata {
    pub host: String,
    pub port: u16,
    pub mount: String,
    pub user: String,
    pub password: String,
    
    // intern
    last_sent: Mutex<Option<String>>,
    last_try: Mutex<Instant>,
    last_success: Mutex<Option<Instant>>,
    retry_count: Mutex<u32>,
}

impl IcecastMetadata {
    pub fn new(
        host: String,
        port: u16,
        mount: String,
        user: String,
        password: String,
    ) -> Self {
        Self {
            host,
            port,
            mount,
            user,
            password,
            last_sent: Mutex::new(None),
            last_try: Mutex::new(Instant::now() - Duration::from_secs(60)),
            last_success: Mutex::new(None),
            retry_count: Mutex::new(0),
        }
    }

    /// Setzt Metadaten mit verbesserter Logik
    pub fn update(&self, text: &str) -> Result<()> {
        // Debounce: gleicher Text → nichts tun
        {
            let last = self.last_sent.lock().unwrap();
            if last.as_deref() == Some(text) {
                return Ok(());
            }
        }

        // Rate limit: min 2 Sekunden zwischen Versuchen
        {
            let mut last_try = self.last_try.lock().unwrap();
            if last_try.elapsed() < Duration::from_secs(2) {
                return Ok(());
            }
            *last_try = Instant::now();
        }

        // Exponential backoff bei Fehlern
        let mut retry_count = self.retry_count.lock().unwrap();
        if *retry_count > 0 {
            let delay_ms = 1000 * 2u64.pow((*retry_count - 1).min(5)); // Max 32s
            warn!("[icecast-meta] Waiting {}ms before retry", delay_ms);
            std::thread::sleep(Duration::from_millis(delay_ms));
        }

        let song = encode(text);
        let addr = match (self.host.as_str(), self.port).to_socket_addrs() {
            Ok(mut addrs) => addrs.next(),
            Err(_) => None,
        };

        let addr = match addr {
            Some(a) => a,
            None => {
                warn!("[icecast-meta] DNS resolve failed");
                *retry_count += 1;
                return Ok(());
            }
        };

        let mut stream = match TcpStream::connect_timeout(&addr, Duration::from_secs(3)) {
            Ok(s) => s,
            Err(e) => {
                warn!("[icecast-meta] Connect failed: {}", e);
                *retry_count += 1;
                return Ok(());
            }
        };

        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;

        let auth = format!("{}:{}", self.user, self.password);
        let auth = general_purpose::STANDARD.encode(auth);

        let req = format!(
            "GET /admin/metadata?mount={}&mode=updinfo&song={} HTTP/1.0\r\n\
             Authorization: Basic {}\r\n\
             User-Agent: Airlift-Node/0.1\r\n\
             \r\n",
            self.mount, song, auth
        );

        if let Err(e) = stream.write_all(req.as_bytes()) {
            warn!("[icecast-meta] Write failed: {}", e);
            *retry_count += 1;
            return Ok(());
        }

        let mut resp = Vec::new();
        let mut buf = [0u8; 1024];
        match stream.read(&mut buf) {
            Ok(n) if n > 0 => resp.extend_from_slice(&buf[..n]),
            Ok(_) => {
                warn!("[icecast-meta] Empty response");
                *retry_count += 1;
                return Ok(());
            }
            Err(e) => {
                warn!("[icecast-meta] Read failed: {}", e);
                *retry_count += 1;
                return Ok(());
            }
        }

        let response = String::from_utf8_lossy(&resp);
        
        // Erfolgreich
        if response.contains("200 OK") {
            info!("[icecast-meta] Updated metadata: '{}'", text);
            *self.last_sent.lock().unwrap() = Some(text.to_string());
            *self.last_success.lock().unwrap() = Some(Instant::now());
            *retry_count = 0;
            return Ok(());
        }

        // Mount existiert noch nicht → normaler Zustand beim Start
        if response.contains("400") && response.contains("Source does not exist") {
            info!("[icecast-meta] Mount not active yet (waiting for audio stream)");
            *retry_count = 0; // Reset, das ist normal
            return Ok(());
        }

        // Unauthorized
        if response.contains("401") || response.contains("403") {
            error!("[icecast-meta] Authentication failed - check credentials");
            *retry_count += 1;
            return Ok(());
        }

        // Alles andere
        warn!("[icecast-meta] Update failed: {}", 
            response.lines().next().unwrap_or("unknown response"));
        *retry_count += 1;

        Ok(())
    }
    
    /// Sendet Standard-Metadata beim Start
    pub fn send_default(&self, default_text: &str) -> Result<()> {
        info!("[icecast-meta] Sending default metadata: '{}'", default_text);
        self.update(default_text)
    }
    
    /// Prüft, ob die Verbindung erfolgreich war
    pub fn is_connected(&self) -> bool {
        let last_success = self.last_success.lock().unwrap();
        last_success.map_or(false, |t| t.elapsed() < Duration::from_secs(60))
    }
}
