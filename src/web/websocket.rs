use axum::{
    extract::{State, WebSocketUpgrade},
    response::Response,
};
use futures::{SinkExt, StreamExt};
use serde::Serialize;
use tokio::sync::broadcast;

use crate::web::WebState;

#[derive(Serialize)]
struct WsPeak {
    timestamp: u64,
    peaks: [f32; 2],
    silence: bool,
}

pub async fn websocket_handler(ws: WebSocketUpgrade, State(state): State<WebState>) -> Response {
    ws.on_upgrade(move |socket| handle(socket, state))
}

async fn handle(mut socket: axum::extract::ws::WebSocket, state: WebState) {
    let mut rx = state.peak_store.subscribe();

    loop {
        tokio::select! {
            Some(msg) = socket.next() => {
                if msg.is_err() {
                    break;
                }
            }
            result = rx.recv() => {
                match result {
                    Ok(peak) => {
                        let payload = WsPeak {
                            timestamp: peak.timestamp,
                            peaks: [peak.peak_l, peak.peak_r],
                            silence: peak.silence,
                        };
                        if let Ok(json) = serde_json::to_string(&payload) {
                            if socket.send(axum::extract::ws::Message::Text(json)).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}
