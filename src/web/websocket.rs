// src/web/websocket.rs
use axum::{
    extract::{State, WebSocketUpgrade},
    response::Response,
};
use futures::StreamExt;
use std::sync::Arc;

use crate::web::peaks::PeakStorage;
use crate::web::influx_service::InfluxService;

pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    State((peak_store, _influx_service)): State<Arc<InfluxService>>
) -> Response {
    ws.on_upgrade(|socket| handle_websocket(socket, peak_store))
}

async fn handle_websocket(mut socket: axum::extract::ws::WebSocket, peak_store: Arc<PeakStorage>) {
    // Sende initiale Daten
    if let Some(peak) = peak_store.get_latest() {
        if let Ok(json) = serde_json::to_string(&peak) {
            let _ = socket.send(axum::extract::ws::Message::Text(json)).await;
        }
    }

    // Halte die Verbindung offen
    while let Some(msg) = socket.next().await {
        match msg {
            Ok(_msg) => {
                // Client-Nachrichten ignorieren oder verarbeiten
            }
            Err(_) => break,
        }
    }
}
