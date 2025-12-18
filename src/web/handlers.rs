use axum::Json;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use futures_util::SinkExt;
use serde::Deserialize;

use crate::io::peak_analyzer::PeakEvent;
use crate::web::peak_storage::PeakBufferInfo;
use crate::web::{AppState, merge_peak_into_state};

pub async fn get_peaks(State(state): State<AppState>) -> Json<Vec<PeakEvent>> {
    Json(state.peaks.snapshot(1000))
}

pub async fn get_history(State(state): State<AppState>) -> Json<Vec<PeakEvent>> {
    Json(state.peaks.snapshot(1000))
}

pub async fn buffer_info(State(state): State<AppState>) -> Json<PeakBufferInfo> {
    Json(state.peaks.info())
}

pub async fn ingest_peak(
    State(state): State<AppState>,
    Json(payload): Json<IncomingPeak>,
) -> impl IntoResponse {
    let evt = payload.into_event();
    merge_peak_into_state(&state, evt);
    StatusCode::NO_CONTENT
}

pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| websocket_task(socket, state))
}

async fn websocket_task(mut socket: WebSocket, state: AppState) {
    if let Some(latest) = state.peaks.get_latest() {
        if let Ok(txt) = serde_json::to_string(&latest) {
            let _ = socket.send(Message::Text(txt)).await;
        }
    }
}

#[derive(Debug, Deserialize)]
struct IncomingPeak {
    #[serde(default)]
    _type: Option<String>,
    seq: u64,
    utc_ns: u64,
    #[serde(rename = "peakL")]
    peak_l: f32,
    #[serde(rename = "peakR")]
    peak_r: f32,
    silence: bool,
    latency_ms: f32,
}

impl IncomingPeak {
    fn into_event(self) -> PeakEvent {
        PeakEvent {
            seq: self.seq,
            utc_ns: self.utc_ns,
            peak_l: self.peak_l,
            peak_r: self.peak_r,
            silence: self.silence,
            latency_ms: self.latency_ms,
        }
    }
}
