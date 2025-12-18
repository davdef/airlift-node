use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::routing::{get, post};

use crate::io::peak_analyzer::PeakEvent;

pub mod handlers;
pub mod influx_service;
pub mod peak_storage;

#[derive(Clone)]
pub struct AppState {
    pub peaks: Arc<peak_storage::PeakStorage>,
    pub influx: Arc<influx_service::InfluxService>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/peaks", get(handlers::get_peaks))
        .route("/api/history", get(handlers::get_history))
        .route("/buffer-info", get(handlers::buffer_info))
        .route("/ws", get(handlers::websocket_handler))
        .route("/api/broadcast", post(handlers::ingest_peak))
        .with_state(state)
}

pub async fn serve(state: AppState, addr: SocketAddr) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let app = router(state);

    axum::serve(listener, app).await?;
    Ok(())
}

pub fn merge_peak_into_state(state: &AppState, peak: PeakEvent) {
    state.peaks.add_peak(peak.clone());
    state.influx.handle_peak(&peak);
}
