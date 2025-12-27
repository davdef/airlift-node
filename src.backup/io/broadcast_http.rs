use crate::io::peak_analyzer::PeakEvent;

use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct BroadcastHttp {
    url: String,
    min_interval: Duration,
    last_send: Mutex<Instant>,
}

impl BroadcastHttp {
    pub fn new(url: String, min_interval_ms: u64) -> Self {
        Self {
            url,
            min_interval: Duration::from_millis(min_interval_ms),
            last_send: Mutex::new(
                Instant::now() - Duration::from_secs(1)
            ),
        }
    }

    pub fn handle(&self, evt: &PeakEvent) {
        let mut last = self.last_send.lock().unwrap();

        if last.elapsed() < self.min_interval {
            return; // 🔥 DROP (gewollt)
        }
        *last = Instant::now();

        let payload = serde_json::json!({
            "type": "peak",
            "seq": evt.seq,
            "utc_ns": evt.utc_ns,
            "peakL": evt.peak_l,
            "peakR": evt.peak_r,
            "silence": evt.silence,
            "latency_ms": evt.latency_ms
        });

        let body = payload.to_string();

        let _ = ureq::post(&self.url)
            .set("Content-Type", "application/json")
            .timeout(Duration::from_millis(200))
            .send_string(&body);
        // Fehler bewusst ignoriert
    }
}
