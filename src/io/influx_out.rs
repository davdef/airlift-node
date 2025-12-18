use crate::io::peak_analyzer::PeakEvent;

use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct InfluxOut {
    url: String,          // z.B. http://localhost:8086/write
    db: String,           // rfm_aircheck
    min_interval: Duration,
    last_send: Mutex<Instant>,
}

impl InfluxOut {
    pub fn new(url: String, db: String, min_interval_ms: u64) -> Self {
        Self {
            url,
            db,
            min_interval: Duration::from_millis(min_interval_ms),
            last_send: Mutex::new(Instant::now() - Duration::from_secs(1)),
        }
    }

    pub fn handle(&self, evt: &PeakEvent) {
        let mut last = self.last_send.lock().unwrap();

        if last.elapsed() < self.min_interval {
            return; // 🔥 DROP (gewollt)
        }
        *last = Instant::now();

        // ---- peaks ----
        let peaks_line = format!(
            "peaks,source=airlift \
             peakL={:.6},peakR={:.6},silence={}i {}",
            evt.peak_l,
            evt.peak_r,
            if evt.silence { 1 } else { 0 },
            evt.utc_ns
        );

        // ---- latency ----
        let latency_line = format!(
            "latency,source=airlift \
             transport_ms={:.3} {}",
            evt.latency_ms,
            evt.utc_ns
        );

        let body = format!("{}\n{}", peaks_line, latency_line);

        let _ = ureq::post(&format!(
                "{}?db={}&precision=ns",
                self.url, self.db
            ))
            .timeout(Duration::from_millis(300))
            .set("Content-Type", "text/plain")
            .send_string(&body);
        // Fehler bewusst ignoriert
    }
}
