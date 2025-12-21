use axum::extract::ws::Message;
use axum::{
    extract::{State, WebSocketUpgrade},
    response::Response,
};
use futures::{SinkExt, StreamExt};
use log::{debug, warn};
use serde::Serialize;
use tokio::sync::broadcast;

use crate::api::ApiState;

#[derive(Serialize)]
struct WsPeak {
    timestamp: u64,
    peaks: [f32; 2],
    silence: bool,
}

pub async fn websocket_handler(ws: WebSocketUpgrade, State(state): State<ApiState>) -> Response {
    ws.on_upgrade(move |socket| handle(socket, state))
}

async fn handle(mut socket: axum::extract::ws::WebSocket, state: ApiState) {
    let mut rx = state.peak_store.subscribe();

    debug!("[ws] client connected");

    loop {
        tokio::select! {
            Some(msg) = socket.next() => {
                match msg {
                    Ok(Message::Ping(payload)) => {
                        if socket.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Ok(Message::Close(frame)) => {
                        debug!("[ws] client closed connection: {:?}", frame);
                        break;
                    }
                    Ok(Message::Pong(_)) => {}
                    Ok(Message::Text(_)) => {}
                    Ok(Message::Binary(_)) => {}
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
