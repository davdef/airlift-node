// src/web/mod.rs
pub mod audio_api;
pub mod influx_service;
pub mod peaks;
pub mod peaks_api;
pub mod player;
pub mod websocket;
pub mod control_api;

use axum::{
    extract::{Query, State},
    response::IntoResponse,
    routing::get,
    Router,
};
use log::info;
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::services::ServeDir;

use crate::web::audio_api::{AudioState, AudioTimestampQuery};
use crate::web::influx_service::InfluxHistoryService;
use crate::web::peaks::PeakStorage;
use crate::control::ControlState;

// Importiere AudioRing, nicht RingBuffer
use crate::ring::AudioRing;

#[derive(Clone)]
pub struct WebState {
    pub peak_store: Arc<PeakStorage>,
    pub history_service: Option<Arc<InfluxHistoryService>>,
    pub audio_state: AudioState,
    pub control_state: Arc<ControlState>,
}

pub async fn run_web_server(
    peak_store: Arc<PeakStorage>,
    history_service: Option<Arc<InfluxHistoryService>>,
    ring_buffer: Arc<AudioRing>,  // AudioRing hier
    wav_dir: PathBuf,
    port: u16,
    control_state: Arc<ControlState>,
) -> anyhow::Result<()> {
    let addr: std::net::SocketAddr = format!("0.0.0.0:{port}").parse()?;

    let audio_state = AudioState {
        wav_dir,
        ring: ring_buffer,
    };

    let app_state = WebState {
        peak_store,
        history_service,
        audio_state,
        control_state,
    };

    async fn handle_live_audio(
        State(state): State<WebState>,
    ) -> impl IntoResponse {
        audio_api::live_audio_stream(State(state.audio_state)).await
    }

    async fn handle_historical_audio(
        State(state): State<WebState>,
        Query(params): Query<AudioTimestampQuery>,
    ) -> impl IntoResponse {
        audio_api::historical_audio_stream(State(state.audio_state), Query(params)).await
    }

    let app = Router::new()
        .route("/api/peaks", get(peaks_api::get_peaks))
        .route("/api/history", get(peaks_api::get_history))
        .route("/api/status", get(control_api::get_status))
        .route("/api/control", axum::routing::post(control_api::post_control))
        .route("/api/events", get(control_api::events))
        .route("/ws", get(websocket::websocket_handler))
        .nest_service("/", ServeDir::new("public").append_index_html_on_directories(true))
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!("[web] server listening on {}", addr);
    axum::serve(listener, app).await?;
    Ok(())
}
