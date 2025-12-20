pub mod influx_service;
pub mod peaks;
pub mod peaks_api;
pub mod player;
pub mod websocket;

use axum::{Router, routing::get};
use std::sync::Arc;

use crate::web::influx_service::InfluxHistoryService;
use crate::web::peaks::PeakStorage;

#[derive(Clone)]
pub struct WebState {
    pub peak_store: Arc<PeakStorage>,
    pub history_service: Option<Arc<InfluxHistoryService>>,
}

pub async fn run_web_server(
    peak_store: Arc<PeakStorage>,
    history_service: Option<Arc<InfluxHistoryService>>,
    port: u16,
) -> anyhow::Result<()> {
    let addr: std::net::SocketAddr = format!("0.0.0.0:{port}").parse()?;

    let app_state = WebState {
        peak_store,
        history_service,
    };

    let app = Router::new()
        .route("/api/peaks", get(peaks_api::get_peaks))
        .route("/api/history", get(peaks_api::get_history))
        .route("/ws", get(websocket::websocket_handler))
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
