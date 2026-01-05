use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tiny_http::{Header, Request, Response, StatusCode};

use crate::core::lock::lock_mutex;
use crate::core::{AirliftNode, EventHandler, EventPriority, EventType};

const PEAK_HISTORY_RETENTION_MS: u64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Serialize)]
pub struct PeakPoint {
    pub ts: u64,
    pub peak_l: f32,
    pub peak_r: f32,
    pub silence: bool,
}

#[derive(Debug)]
pub struct PeakHistory {
    points: VecDeque<PeakPoint>,
}

impl PeakHistory {
    pub fn new() -> Self {
        Self {
            points: VecDeque::new(),
        }
    }

    pub fn push(&mut self, point: PeakPoint) {
        self.points.push_back(point);
        self.trim_to_retention();
    }

    pub fn range(&self, from: u64, to: u64) -> Vec<PeakPoint> {
        self.points
            .iter()
            .filter(|point| point.ts >= from && point.ts <= to)
            .cloned()
            .collect()
    }

    pub fn buffer_range(&self) -> Option<(u64, u64)> {
        let start = self.points.front()?.ts;
        let end = self.points.back()?.ts;
        Some((start, end))
    }

    fn trim_to_retention(&mut self) {
        if let Some(latest) = self.points.back().map(|point| point.ts) {
            let min_ts = latest.saturating_sub(PEAK_HISTORY_RETENTION_MS);
            while let Some(front) = self.points.front() {
                if front.ts < min_ts {
                    self.points.pop_front();
                } else {
                    break;
                }
            }
        }
    }
}

pub struct PeakHistoryHandler {
    name: String,
    history: Arc<Mutex<PeakHistory>>,
}

impl PeakHistoryHandler {
    pub fn new(name: impl Into<String>, history: Arc<Mutex<PeakHistory>>) -> Self {
        Self {
            name: name.into(),
            history,
        }
    }
}

impl EventHandler for PeakHistoryHandler {
    fn handle_event(&self, event: &crate::core::Event) -> anyhow::Result<()> {
        let payload = &event.payload;
        let timestamp = payload.get("timestamp").and_then(normalize_timestamp_ms);
        let peaks = payload.get("peaks").and_then(|value| value.as_array());

        let (Some(timestamp), Some(peaks)) = (timestamp, peaks) else {
            return Ok(());
        };

        let peak_l = peaks.get(0).and_then(|value| value.as_f64()).unwrap_or(0.0) as f32;
        let peak_r = peaks.get(1).and_then(|value| value.as_f64()).unwrap_or(peak_l as f64) as f32;
        let silence = payload
            .get("silence")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);

        let mut history = lock_mutex(&self.history, "api.peak_history.push");
        history.push(PeakPoint {
            ts: timestamp,
            peak_l,
            peak_r,
            silence,
        });

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

pub fn register_peak_history(node: Arc<Mutex<AirliftNode>>) -> Arc<Mutex<PeakHistory>> {
    let history = Arc::new(Mutex::new(PeakHistory::new()));
    let handler = Arc::new(PeakHistoryHandler::new(
        "api_peak_history",
        history.clone(),
    ));
    let event_bus = {
        let node = lock_mutex(&node, "api.peak_history.node");
        node.event_bus()
    };

    let mut bus = lock_mutex(&event_bus, "api.peak_history.register_handler");
    if let Err(error) = bus.register_handler(handler) {
        log::error!("Failed to register peak history handler: {}", error);
    }

    history
}

pub fn handle_peaks_request(request: Request, history: Arc<Mutex<PeakHistory>>) {
    #[derive(Serialize)]
    struct PeaksResponse {
        ok: bool,
        start: Option<u64>,
        end: Option<u64>,
    }

    let range = {
        let history = lock_mutex(&history, "api.peak_history.range");
        history.buffer_range()
    };

    let response = PeaksResponse {
        ok: range.is_some(),
        start: range.map(|(start, _)| start),
        end: range.map(|(_, end)| end),
    };

    respond_json(request, StatusCode(200), response);
}

pub fn handle_history_request(
    request: Request,
    history: Arc<Mutex<PeakHistory>>,
    query: Option<&str>,
) {
    let Some((from, to)) = parse_history_query(query) else {
        let _ = request.respond(Response::empty(StatusCode(400)));
        return;
    };

    let points = {
        let history = lock_mutex(&history, "api.peak_history.query");
        history.range(from, to)
    };

    respond_json(request, StatusCode(200), points);
}

fn respond_json<T: Serialize>(request: Request, status: StatusCode, payload: T) {
    let body = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
    let response = Response::from_string(body).with_status_code(status).with_header(
        Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
    );
    let _ = request.respond(response);
}

fn parse_history_query(query: Option<&str>) -> Option<(u64, u64)> {
    let query = query?;
    let from = query_value(query, "from")?.parse::<u64>().ok()?;
    let to = query_value(query, "to")?.parse::<u64>().ok()?;
    if from >= to {
        return None;
    }
    Some((from, to))
}

fn query_value<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    query.split('&').find_map(|pair| {
        let mut iter = pair.splitn(2, '=');
        let name = iter.next()?;
        let value = iter.next()?;
        if name == key {
            Some(value)
        } else {
            None
        }
    })
}

fn normalize_timestamp_ms(value: &serde_json::Value) -> Option<u64> {
    let raw = value
        .as_u64()
        .or_else(|| value.as_i64().and_then(|val| u64::try_from(val).ok()))
        .or_else(|| value.as_f64().map(|val| val as u64))?;

    if raw > 1_000_000_000_000_000 {
        Some(raw / 1_000_000)
    } else if raw > 1_000_000_000_000 {
        Some(raw / 1_000)
    } else {
        Some(raw)
    }
}
