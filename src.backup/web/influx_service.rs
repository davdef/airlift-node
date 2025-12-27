// src/web/influx_service.rs - VOLLSTÄNDIG KORRIGIERT FÜR INFLUXDB 1.x

use log::{debug, warn};
use serde::Serialize;
use std::collections::HashMap;

//
// ============================================================
// WRITE SIDE – Peak → Influx
// ============================================================
//

pub struct InfluxService {
    inner: crate::io::influx_out::InfluxOut,
}

impl InfluxService {
    pub fn new(url: String, db: String, min_interval_ms: u64) -> Self {
        Self {
            inner: crate::io::influx_out::InfluxOut::new(url, db, min_interval_ms),
        }
    }

    pub fn handle_peak(&self, evt: &crate::io::peak_analyzer::PeakEvent) {
        self.inner.handle(evt);
    }
}

//
// ============================================================
// READ SIDE – History API (InfluxDB 1.x)
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
        let from_ns = from_ms.saturating_mul(1_000_000);
        let to_ns = to_ms.saturating_mul(1_000_000);
        // INFLUXQL QUERY für InfluxDB 1.x
        let query = format!(
            "SELECT peakL, peakR, silence FROM peaks WHERE time >= {} AND time <= {}",
            from_ns, to_ns
        );

        let query_url = format!("{}/query", self.base_url);
        
        let response = match ureq::post(&query_url)
            .query("db", &self.bucket)  // DB Parameter für 1.x
            .query("q", &query)         // Query Parameter
            .set("Accept", "application/json")
            .call() {
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

        let json_text = response.into_string()
            .map_err(|e| format!("Read error: {}", e))?;
        
        let json: serde_json::Value = serde_json::from_str(&json_text)
            .map_err(|e| format!("JSON parse error: {}", e))?;

        let points = parse_influxql_json(&json)?;
        debug!("[influx] history query returned {} points", points.len());
        Ok(points)
    }
}

//
// ============================================================
// Helpers
// ============================================================
//

fn parse_influxql_json(json: &serde_json::Value) -> Result<Vec<HistoryPoint>, String> {
    let mut points = Vec::new();
    
    if let Some(results) = json.get("results").and_then(|r| r.as_array()) {
        for result in results {
            if let Some(series) = result.get("series").and_then(|s| s.as_array()) {
                for serie in series {
                    let columns = serie.get("columns")
                        .and_then(|c| c.as_array())
                        .ok_or("No columns")?;
                    
                    let values = serie.get("values")
                        .and_then(|v| v.as_array())
                        .ok_or("No values")?;
                    
                    // Finde Spaltenindizes
                    let time_idx = columns.iter().position(|c| c.as_str() == Some("time"))
                        .ok_or("No time column")?;
                    let peakl_idx = columns.iter().position(|c| c.as_str() == Some("peakL"))
                        .ok_or("No peakL column")?;
                    let peakr_idx = columns.iter().position(|c| c.as_str() == Some("peakR"))
                        .ok_or("No peakR column")?;
                    let silence_idx = columns.iter().position(|c| c.as_str() == Some("silence"))
                        .ok_or("No silence column")?;
                    
                    for row in values {
                        if let Some(row_arr) = row.as_array() {
                            if row_arr.len() > silence_idx {
                                let timestamp_str = row_arr[time_idx].as_str()
                                    .ok_or("Invalid timestamp")?;
                                
                                // Konvertiere RFC3339 zu ms
                                let ts = parse_rfc3339_to_ms(timestamp_str)?;
                                
                                let peak_l = row_arr[peakl_idx].as_f64()
                                    .unwrap_or(0.0) as f32;
                                let peak_r = row_arr[peakr_idx].as_f64()
                                    .unwrap_or(0.0) as f32;
                                let silence = row_arr[silence_idx].as_i64()
                                    .map(|v| v == 1)
                                    .unwrap_or(false);
                                
                                points.push(HistoryPoint {
                                    ts,
                                    peak_l,
                                    peak_r,
                                    silence,
                                });
                            }
                        }
                    }
                }
            } else {
                warn!("[influx] query returned no series");
            }
        }
    } else {
        warn!("[influx] query returned no results array");
    }
    
    points.sort_by_key(|p| p.ts);
    Ok(points)
}

fn parse_rfc3339_to_ms(s: &str) -> Result<u64, String> {
    let s = s.trim_end_matches('Z');
    let (base, frac) = s.split_once('.').unwrap_or((s, "0"));

    let parsed = chrono::NaiveDateTime::parse_from_str(base, "%Y-%m-%dT%H:%M:%S")
        .map_err(|e| e.to_string())?;

    let nanos: i64 = frac.chars().take(9).collect::<String>().parse().unwrap_or(0);

    Ok((chrono::Duration::seconds(parsed.and_utc().timestamp())  // FIXED: nicht deprecated
        + chrono::Duration::nanoseconds(nanos))
    .num_milliseconds() as u64)
}
