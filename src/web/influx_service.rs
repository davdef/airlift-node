use crate::io::influx_out::InfluxOut;
use crate::io::peak_analyzer::PeakEvent;

use serde::Serialize;
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};

//
// ============================================================
// WRITE SIDE – Peak → Influx
// ============================================================
//

pub struct InfluxService {
    inner: InfluxOut,
}

impl InfluxService {
    pub fn new(url: String, db: String, min_interval_ms: u64) -> Self {
        Self {
            inner: InfluxOut::new(url, db, min_interval_ms),
        }
    }

    pub fn handle_peak(&self, evt: &PeakEvent) {
        self.inner.handle(evt);
    }
}

//
// ============================================================
// READ SIDE – History API (InfluxDB 2.x)
// ============================================================
//

#[derive(Debug, Clone, Serialize)]
pub struct HistoryPoint {
    pub ts: u64, // milliseconds
    pub peak_l: f32,
    pub peak_r: f32,
    pub silence: bool,
}

#[derive(Clone)]
pub struct InfluxHistoryService {
    base_url: String,
    token: String,
    org: String,
    bucket: String,
}

impl InfluxHistoryService {
    pub fn new(base_url: String, token: String, org: String, bucket: String) -> Self {
        let base_url = base_url.trim_end_matches('/').to_string();
        Self {
            base_url,
            token,
            org,
            bucket,
        }
    }

    pub fn get_history(
        &self,
        from_ms: u64,
        to_ms: u64,
    ) -> Result<Vec<HistoryPoint>, String> {
        let flux_query = format!(
            r#"
            from(bucket: "{}")
              |> range(start: {}, stop: {})
              |> filter(fn: (r) => r._measurement == "peaks")
              |> pivot(rowKey: ["_time"], columnKey: ["_field"], valueColumn: "_value")
              |> keep(columns: ["_time", "peakL", "peakR", "silence"])
            "#,
            self.bucket,
            from_ms * 1_000_000,
            to_ms * 1_000_000
        );

        let query_url = format!("{}/api/v2/query?org={}", self.base_url, self.org);

        let response = match ureq::post(&query_url)
            .set("Authorization", &format!("Token {}", self.token))
            .set("Content-Type", "application/json")
            .set("Accept", "application/csv")
            .send_string(
                &serde_json::to_string(&json!({
                    "query": flux_query,
                    "type": "flux"
                }))
                .unwrap(),
            ) {
            Ok(resp) => resp,
            Err(ureq::Error::Status(status, resp)) => {
                return Err(format!(
                    "InfluxDB error {}: {}",
                    status,
                    resp.into_string().unwrap_or_default()
                ));
            }
            Err(e) => return Err(format!("Request error: {}", e)),
        };

        let csv = response
            .into_string()
            .map_err(|e| format!("Read error: {}", e))?;

        parse_flux_csv(&csv)
    }
}

//
// ============================================================
// Helpers
// ============================================================
//

fn parse_flux_csv(csv_text: &str) -> Result<Vec<HistoryPoint>, String> {
    use std::collections::HashMap;

    let mut map: HashMap<u64, HistoryPoint> = HashMap::new();

    for line in csv_text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() < 9 {
            continue;
        }

        let ts = parse_rfc3339_to_ms(parts[4].trim_matches('"'))?;
        let field = parts[5].trim_matches('"');
        let value = parts[8].trim_matches('"').parse::<f64>().unwrap_or(0.0);

        let entry = map.entry(ts).or_insert(HistoryPoint {
            ts,
            peak_l: 0.0,
            peak_r: 0.0,
            silence: false,
        });

        match field {
            "peakL" => entry.peak_l = value as f32,
            "peakR" => entry.peak_r = value as f32,
            "silence" => entry.silence = value == 1.0,
            _ => {}
        }
    }

    let mut out: Vec<_> = map.into_values().collect();
    out.sort_by_key(|p| p.ts);
    Ok(out)
}

fn parse_rfc3339_to_ms(s: &str) -> Result<u64, String> {
    let s = s.trim_end_matches('Z');
    let (base, frac) = s.split_once('.').unwrap_or((s, "0"));

    let parsed =
        chrono::NaiveDateTime::parse_from_str(base, "%Y-%m-%dT%H:%M:%S")
            .map_err(|e| e.to_string())?;

    let nanos: i64 = frac.chars().take(9).collect::<String>().parse().unwrap_or(0);

    Ok((chrono::Duration::seconds(parsed.timestamp())
        + chrono::Duration::nanoseconds(nanos))
    .num_milliseconds() as u64)
}
