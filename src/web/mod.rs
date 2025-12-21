// src/web/mod.rs
pub mod audio_api;
pub mod influx_service;
pub mod peaks;
pub mod peaks_api;
pub mod player;
pub mod websocket;

use axum::{
    extract::{Query, State},
    response::IntoResponse,
    routing::get,
    Router,
};
use log::info;
use std::path::PathBuf;
use std::sync::Arc;

use crate::web::audio_api::{AudioState, AudioTimestampQuery};
use crate::web::influx_service::InfluxHistoryService;
use crate::web::peaks::PeakStorage;

// Importiere AudioRing, nicht RingBuffer
use crate::ring::AudioRing;

#[derive(Clone)]
pub struct WebState {
    pub peak_store: Arc<PeakStorage>,
    pub history_service: Option<Arc<InfluxHistoryService>>,
    pub audio_state: AudioState,
}

pub async fn run_web_server(
    peak_store: Arc<PeakStorage>,
    history_service: Option<Arc<InfluxHistoryService>>,
    ring_buffer: Arc<AudioRing>,  // AudioRing hier
    wav_dir: PathBuf,
    port: u16,
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
        .route("/ws", get(websocket::websocket_handler))
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!("[web] server listening on {}", addr);
    axum::serve(listener, app).await?;
    Ok(())
}
