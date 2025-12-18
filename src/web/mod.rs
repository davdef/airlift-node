<<<<<<< HEAD
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
=======
// src/web/mod.rs
pub mod peaks;
pub mod player;
pub mod websocket;
pub mod influx_service;

use axum::{
    Router,
    routing::get,
    extract::{State, Query},
    response::Json,
};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use serde::Deserialize;
use serde_json::json;

use crate::web::peaks::PeakStorage;
use crate::web::influx_service::{InfluxService, HistoryPoint};

// Globaler InfluxService
use once_cell::sync::Lazy;
static INFLUX_SERVICE: Lazy<Arc<InfluxService>> = Lazy::new(|| {
    Arc::new(InfluxService::new(
        "http://localhost:8086".to_string(),
        "".to_string(), // Token falls benötigt
        "rfm_aircheck".to_string(),
        "rfm_aircheck".to_string(),
    ))
});

#[derive(Deserialize)]
struct HistoryQuery {
    from: Option<u64>,
    to: Option<u64>,
}

pub async fn run_web_server(
    peak_store: Arc<PeakStorage>, 
    port: u16
) -> anyhow::Result<()> {
    let addr: std::net::SocketAddr = format!("0.0.0.0:{}", port).parse().unwrap();
    
    let app = Router::new()
        .route("/", get(player::index))
        .route("/player", get(player::index))
        .route("/api/peaks", get(peaks::get_peaks))
        .route("/buffer-info", get(buffer_info_handler))
        .route("/api/history", get(history_handler))
        .route("/ws", get(websocket::websocket_handler))
        .with_state(peak_store);  // NUR PeakStorage!
    
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("Web server listening on {}", addr);
    axum::serve(listener, app).await?;
    
    Ok(())
}

// Handler - alle nehmen nur PeakStorage State
async fn buffer_info_handler(
    State(peak_store): State<Arc<PeakStorage>>
) -> Json<serde_json::Value> {
    let (_, buffer_len) = peak_store.get_buffer_bounds();
    
    let buffer = match peak_store.get_buffer_read_lock() {
        Ok(buffer) => buffer,
        Err(_) => return Json(json!({"ok": false, "error": "buffer locked"}))
    };
    
    let start_ms = buffer.front()
        .map(|p| p.timestamp / 1_000_000)
        .unwrap_or(0);
    let end_ms = buffer.back()
        .map(|p| p.timestamp / 1_000_000)
        .unwrap_or(0);
    
    Json(json!({
        "ok": true,
        "start": start_ms,
        "end": end_ms,
        "size": buffer_len
    }))
}

async fn history_handler(
    Query(params): Query<HistoryQuery>,
    State(_peak_store): State<Arc<PeakStorage>>  // Wird nicht gebraucht, aber muss da sein
) -> Json<Vec<HistoryPoint>> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    
    let from = params.from.unwrap_or_else(|| now.saturating_sub(5 * 60 * 1000));
    let to = params.to.unwrap_or(now);
    
    match INFLUX_SERVICE.get_history(from, to) {
        Ok(points) => {
            println!("[History] Returning {} points", points.len());
            Json(points)
        }
        Err(e) => {
            eprintln!("[History] Error: {}", e);
            Json(vec![])
        }
    }
>>>>>>> ffd6f69 (Frontend integration)
}
