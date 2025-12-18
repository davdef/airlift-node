use axum::{
    extract::{State, WebSocketUpgrade},
    response::Response,
};
use futures::StreamExt;
use std::sync::Arc;

use crate::web::peaks::PeakStorage;

pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(peak_store): State<Arc<PeakStorage>>,
) -> Response {
    ws.on_upgrade(move |socket| handle(socket, peak_store))
}

async fn handle(
    mut socket: axum::extract::ws::WebSocket,
    peak_store: Arc<PeakStorage>,
) {
    if let Some(peak) = peak_store.get_latest() {
        if let Ok(json) = serde_json::to_string(&peak) {
            let _ = socket
                .send(axum::extract::ws::Message::Text(json))
                .await;
        }
    }

    while socket.next().await.is_some() {}
}
