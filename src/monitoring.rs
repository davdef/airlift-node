// src/monitoring.rs
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::net::{TcpListener, TcpStream};
use std::io::{Write, Read};
use std::time::{Duration, Instant};
use std::thread;
use crate::ring::AudioRing;

#[derive(Debug)]
pub struct Metrics {
    pub alsa_samples: AtomicU64,
    pub icecast_packets: AtomicU64,
    pub udp_packets: AtomicU64,
    pub gaps_total: AtomicU64,
    pub bytes_sent: AtomicU64,
    pub start_time: Instant,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            alsa_samples: AtomicU64::new(0),
            icecast_packets: AtomicU64::new(0),
            udp_packets: AtomicU64::new(0),
            gaps_total: AtomicU64::new(0),
            bytes_sent: AtomicU64::new(0),
            start_time: Instant::now(),
        }
    }
}

impl Metrics {
    pub fn new() -> Self {
        Self::default()
    }
    
    pub fn uptime(&self) -> Duration {
        self.start_time.elapsed()
    }
    
    pub fn to_json(&self) -> String {
        format!(
            r#"{{
  "alsa_samples": {},
  "icecast_packets": {},
  "udp_packets": {},
  "gaps_total": {},
  "bytes_sent": {},
  "uptime_seconds": {:.2},
  "status": "running"
}}"#,
            self.alsa_samples.load(Ordering::Relaxed),
            self.icecast_packets.load(Ordering::Relaxed),
            self.udp_packets.load(Ordering::Relaxed),
            self.gaps_total.load(Ordering::Relaxed),
            self.bytes_sent.load(Ordering::Relaxed),
            self.uptime().as_secs_f64()
        )
    }
}

pub fn run_metrics_server(
    metrics: Arc<Metrics>, 
    ring: AudioRing,
    port: u16,
    running: Arc<std::sync::atomic::AtomicBool>
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(("0.0.0.0", port))?;
    listener.set_nonblocking(true)?;
    
    println!("[monitoring] Metrics server listening on port {}", port);
    
    while running.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, addr)) => {
                let metrics = metrics.clone();
                let ring = ring.clone();
                let running = running.clone();
                
                thread::spawn(move || {
                    if let Err(e) = handle_client(stream, metrics, ring, running) {
                        eprintln!("[monitoring] Client {} error: {}", addr, e);
                    }
                });
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                eprintln!("[monitoring] Accept error: {}", e);
                thread::sleep(Duration::from_millis(1000));
            }
        }
    }
    
    println!("[monitoring] Metrics server shutdown");
    Ok(())
}

fn handle_client(
    mut stream: TcpStream, 
    metrics: Arc<Metrics>,
    ring: AudioRing,
    running: Arc<std::sync::atomic::AtomicBool>
) -> anyhow::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    
    let mut buffer = [0; 1024];
    let n = stream.read(&mut buffer)?;
    let request = String::from_utf8_lossy(&buffer[..n]);
    
    let response = if request.contains("GET /metrics") {
        metrics.to_json()
    } else if request.contains("GET /health") {
        let stats = ring.stats();
        let fill = stats.head_seq - stats.next_seq.wrapping_sub(1);
        
        if running.load(Ordering::Relaxed) && fill < 100 {
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nOK".to_string()
        } else {
            "HTTP/1.1 503 Service Unavailable\r\nContent-Type: text/plain\r\n\r\nSERVICE UNAVAILABLE".to_string()
        }
    } else if request.contains("GET /stats") {
        let stats = ring.stats();
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{}",
            serde_json::json!({
                "ring": {
                    "capacity": stats.capacity,
                    "head_seq": stats.head_seq,
                    "next_seq": stats.next_seq,
                    "fill": stats.head_seq - stats.next_seq.wrapping_sub(1)
                },
                "metrics": {
                    "alsa_samples": metrics.alsa_samples.load(Ordering::Relaxed),
                    "icecast_packets": metrics.icecast_packets.load(Ordering::Relaxed),
                    "udp_packets": metrics.udp_packets.load(Ordering::Relaxed)
                }
            })
        )
    } else {
        "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain\r\n\r\nNot Found".to_string()
    };
    
    stream.write_all(response.as_bytes())?;
    stream.flush()?;
    
    Ok(())
}

pub fn create_health_file() -> anyhow::Result<()> {
    let path = "/tmp/airlift-health";
    std::fs::write(path, "HEALTHY")?;
    Ok(())
}

pub fn update_health_status(healthy: bool) -> anyhow::Result<()> {
    let status = if healthy { "HEALTHY" } else { "UNHEALTHY" };
    let path = "/tmp/airlift-health";
    std::fs::write(path, status)?;
    Ok(())
}
