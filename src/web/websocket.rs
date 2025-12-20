use axum::{
    extract::{State, WebSocketUpgrade},
    response::Response,
};
use futures::{SinkExt, StreamExt};
use log::{debug, warn};
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

    debug!("[ws] client connected");

    loop {
        tokio::select! {
            Some(msg) = socket.next() => {
                match msg {
                    Ok(axum::extract::ws::Message::Close(frame)) => {
                        debug!("[ws] client closed connection: {:?}", frame);
                        break;
                    }
                    Ok(_) => {}
                    Err(err) => {
                        warn!("[ws] receive error: {}", err);
                        break;
                    }
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
                            if let Err(err) = socket.send(axum::extract::ws::Message::Text(json)).await {
                                warn!("[ws] send error: {}", err);
                                break;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        debug!("[ws] skipped {} messages due to lag", skipped);
                        continue;
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        warn!("[ws] peak broadcast closed");
                        break;
                    }
                }
            }
        }
    }

    debug!("[ws] client disconnected");
}
