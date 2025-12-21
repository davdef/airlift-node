use axum::{
    extract::State,
    response::sse::{Event, Sse},
    Json,
};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::sync::Arc;
use tokio::time::{interval, Duration};
use tokio_stream::{wrappers::IntervalStream, StreamExt};

use crate::control::{ControlState, ModuleSnapshot, now_ms};
use crate::ring::RingStats;
use crate::web::WebState;

#[derive(Serialize)]
pub struct RingStatus {
    pub capacity: usize,
    pub head_seq: u64,
    pub next_seq: u64,
    pub fill: u64,
}

#[derive(Serialize)]
pub struct StatusResponse {
    pub timestamp_ms: u64,
    pub ring: RingStatus,
    pub ring_module: ModuleSnapshot,
    pub srt_in: ModuleSnapshot,
    pub srt_out: ModuleSnapshot,
    pub alsa_in: ModuleSnapshot,
    pub icecast_out: ModuleSnapshot,
    pub recorder: ModuleSnapshot,
}

#[derive(Deserialize)]
pub struct ControlRequest {
    pub action: String,
}

#[derive(Serialize)]
pub struct ControlResponse {
    pub ok: bool,
    pub message: String,
}

pub async fn get_status(State(state): State<WebState>) -> Json<StatusResponse> {
    Json(build_status(&state.control_state, &state.audio_state.ring.stats()))
}

pub async fn post_control(
    State(state): State<WebState>,
    Json(payload): Json<ControlRequest>,
) -> Json<ControlResponse> {
    let (ok, message) = handle_action(&state.control_state, payload.action.trim());

    Json(ControlResponse {
        ok,
        message: message.to_string(),
    })
}

pub async fn events(State(state): State<WebState>) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let control_state = state.control_state.clone();
    let ring = state.audio_state.ring.clone();

    let stream = IntervalStream::new(interval(Duration::from_secs(1))).map(move |_| {
        let status = build_status(&control_state, &ring.stats());
        let data = serde_json::to_string(&status).unwrap_or_else(|_| "{}".to_string());
        Ok(Event::default().event("status").data(data))
    });

    Sse::new(stream)
}

fn build_status(control_state: &Arc<ControlState>, ring_stats: &RingStats) -> StatusResponse {
    let fill = ring_stats.head_seq.saturating_sub(ring_stats.next_seq.wrapping_sub(1));

    StatusResponse {
        timestamp_ms: now_ms(),
        ring: RingStatus {
            capacity: ring_stats.capacity,
            head_seq: ring_stats.head_seq,
            next_seq: ring_stats.next_seq,
            fill,
        },
        ring_module: control_state.ring.snapshot(),
        srt_in: control_state.srt_in.module.snapshot(),
        srt_out: control_state.srt_out.module.snapshot(),
        alsa_in: control_state.alsa_in.snapshot(),
        icecast_out: control_state.icecast_out.snapshot(),
        recorder: control_state.recorder.snapshot(),
    }
}

fn handle_action(control_state: &Arc<ControlState>, action: &str) -> (bool, &'static str) {
    match action {
        "srt_in.force_disconnect" => {
            control_state.srt_in.force_disconnect.store(true, std::sync::atomic::Ordering::Relaxed);
            (true, "SRT-IN disconnect requested")
        }
        "srt_out.reconnect" => {
            control_state.srt_out.force_reconnect.store(true, std::sync::atomic::Ordering::Relaxed);
            (true, "SRT-OUT reconnect requested")
        }
        "reset_counters" => {
            control_state.reset_counters();
            (true, "Counters reset")
        }
        _ => (false, "Unknown action"),
    }
}
