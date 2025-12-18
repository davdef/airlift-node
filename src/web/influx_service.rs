<<<<<<< HEAD
use crate::io::influx_out::InfluxOut;
use crate::io::peak_analyzer::PeakEvent;

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
=======
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use serde_json::json;

#[derive(Debug, Clone, Serialize)]
pub struct HistoryPoint {
    pub ts: u64,           // timestamp in MILLISEKUNDEN
    pub peak_l: f32,
    pub peak_r: f32,
    pub silence: bool,
}

#[derive(Clone)]
pub struct InfluxService {
    base_url: String,
    token: String,
    org: String,
    bucket: String,
}

impl InfluxService {
    pub fn new(base_url: String, token: String, org: String, bucket: String) -> Self {
        // Sicherstellen, dass keine trailing slash
        let base_url = base_url.trim_end_matches('/').to_string();
        Self { base_url, token, org, bucket }
    }
    
    pub fn get_history(&self, from_ms: u64, to_ms: u64) -> Result<Vec<HistoryPoint>, String> {
        // Flux Query für peaks measurement
        let flux_query = format!(
            r#"
            from(bucket: "{}")
              |> range(start: {}, stop: {})
              |> filter(fn: (r) => r._measurement == "peaks")
              |> pivot(rowKey: ["_time"], columnKey: ["_field"], valueColumn: "_value")
              |> keep(columns: ["_time", "peakL", "peakR", "silence"])
            "#,
            self.bucket,
            // InfluxDB erwartet nanoseconds since epoch
            from_ms * 1_000_000,
            to_ms * 1_000_000
        );
        
        let query_url = format!("{}/api/v2/query?org={}", self.base_url, self.org);
        
        println!("[Influx] Querying: {}ms to {}ms", from_ms, to_ms);
        
        // Flux Query an InfluxDB 2.x senden
let response = match ureq::post(&query_url)
    .set("Authorization", &format!("Token {}", self.token))
    .set("Content-Type", "application/json")
    .set("Accept", "application/csv")
    .send_string(&serde_json::to_string(&json!({
        "query": flux_query,
        "type": "flux",
        "dialect": {
            "header": true,
            "delimiter": ",",
            "commentPrefix": "#",
            "annotations": ["datatype", "group", "default"]
        }
    })).unwrap()) {
            Ok(resp) => resp,
            Err(ureq::Error::Status(status, resp)) => {
                let error_body = resp.into_string().unwrap_or_default();
                return Err(format!("InfluxDB Error {}: {}", status, error_body));
            }
            Err(e) => return Err(format!("Request error: {}", e)),
        };
        
        // CSV parsen
        let csv_text = response.into_string()
            .map_err(|e| format!("Failed to read response: {}", e))?;
        
        self.parse_flux_csv(&csv_text)
    }
    
    fn parse_flux_csv(&self, csv_text: &str) -> Result<Vec<HistoryPoint>, String> {
        use std::collections::HashMap;
        
        let mut points_by_time: HashMap<String, HistoryPoint> = HashMap::new();
        let lines: Vec<&str> = csv_text.lines().collect();
        
        if lines.is_empty() {
            return Ok(Vec::new());
        }
        
        // Finde Spaltenindizes
        let header = lines[0];
        let columns: Vec<&str> = header.split(',').collect();
        
        let time_idx = columns.iter().position(|&c| c == "_time").unwrap_or(4);
        let field_idx = columns.iter().position(|&c| c == "_field").unwrap_or(5);
        let value_idx = columns.iter().position(|&c| c == "_value").unwrap_or(8);
        
        // Datenzeilen verarbeiten (ab Zeile 1, da Zeile 0 Header ist)
        for line in lines.iter().skip(1) {
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            
            let parts: Vec<&str> = line.split(',').collect();
            if parts.len() <= value_idx {
                continue;
            }
            
            let time_str = parts[time_idx].trim_matches('"');
            let field = parts[field_idx].trim_matches('"');
            let value_str = parts[value_idx].trim_matches('"');
            
            // RFC3339 Zeit parsen (z.B. "2024-12-18T19:30:00.123456Z")
            let timestamp_ms = match parse_rfc3339_to_ms(time_str) {
                Ok(ts) => ts,
                Err(_) => continue,
            };
            
            let value: f64 = match value_str.parse() {
                Ok(v) => v,
                Err(_) => continue,
            };
            
            let key = timestamp_ms.to_string();
            let point = points_by_time.entry(key.clone())
                .or_insert_with(|| HistoryPoint {
                    ts: timestamp_ms,
                    peak_l: 0.0,
                    peak_r: 0.0,
                    silence: false,
                });
            
            match field {
                "peakL" => point.peak_l = value as f32,
                "peakR" => point.peak_r = value as f32,
                "silence" => point.silence = value == 1.0,
                _ => {}
            }
        }
        
        let mut points: Vec<HistoryPoint> = points_by_time.into_values().collect();
        points.sort_by_key(|p| p.ts);
        
        println!("[Influx] Parsed {} points", points.len());
        Ok(points)
    }
}

// RFC3339 zu Millisekunden Parser
fn parse_rfc3339_to_ms(rfc3339: &str) -> Result<u64, String> {
    // Vereinfachter Parser - für Produktion chrono crate verwenden
    // Format: 2024-12-18T19:30:00.123456Z
    
    // Entferne das 'Z' am Ende
    let s = rfc3339.trim_end_matches('Z');
    
    // Versuche, Datum und Zeit zu trennen
    if let Some(dot_idx) = s.find('.') {
        let before_dot = &s[..dot_idx];
        let after_dot = &s[dot_idx + 1..];
        
        // Parse den Datum/Zeit Teil (ohne Nanosekunden)
        if let Ok(parsed) = chrono::NaiveDateTime::parse_from_str(before_dot, "%Y-%m-%dT%H:%M:%S") {
            let nanos: u32 = after_dot.chars()
                .take(9)
                .collect::<String>()
                .parse()
                .unwrap_or(0);
            
            let duration = chrono::Duration::seconds(parsed.timestamp())
                + chrono::Duration::nanoseconds(nanos as i64);
            
            return Ok((duration.num_milliseconds() as u64));
        }
    }
    
    // Fallback: Aktuelle Zeit
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64)
}
>>>>>>> ffd6f69 (Frontend integration)
