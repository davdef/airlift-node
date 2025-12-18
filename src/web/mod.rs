pub mod peaks;
pub mod peaks_api;
pub mod websocket;
pub mod player;
pub mod influx_service;

use axum::{routing::get, Router};
use std::sync::Arc;

use crate::web::peaks::PeakStorage;

pub async fn run_web_server(
    peak_store: Arc<PeakStorage>,
    port: u16,
) -> anyhow::Result<()> {
    let addr: std::net::SocketAddr =
        format!("0.0.0.0:{port}").parse()?;

    let app = Router::new()
        .route("/api/peaks", get(peaks_api::get_peaks))
        .route("/ws", get(websocket::websocket_handler))
        .with_state(peak_store);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
