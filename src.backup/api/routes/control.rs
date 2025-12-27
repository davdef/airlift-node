use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};

use crate::api::ApiState;
use crate::control::ControlState;

#[derive(Deserialize)]
pub struct ControlRequest {
    pub action: String,
}

#[derive(Serialize)]
pub struct ControlResponse {
    pub ok: bool,
    pub message: String,
}

pub async fn post_control(
    State(state): State<ApiState>,
    Json(payload): Json<ControlRequest>,
) -> Json<ControlResponse> {
    let (ok, message) = handle_action(&state.control_state, payload.action.trim());

    Json(ControlResponse {
        ok,
        message: message.to_string(),
    })
}

fn handle_action(control_state: &ControlState, action: &str) -> (bool, &'static str) {
    match action {
        "srt_in.force_disconnect" => {
            control_state
                .srt_in
                .force_disconnect
                .store(true, std::sync::atomic::Ordering::Relaxed);
            (true, "SRT-IN disconnect requested")
        }
        "srt_out.reconnect" => {
            control_state
                .srt_out
                .force_reconnect
                .store(true, std::sync::atomic::Ordering::Relaxed);
            (true, "SRT-OUT reconnect requested")
        }
        "reset_counters" => {
            control_state.reset_counters();
            (true, "Counters reset")
        }
        _ => (false, "Unknown action"),
    }
}
