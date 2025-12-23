use axum::{Router, routing::get};
use log::{error, info};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::services::ServeDir;

use crate::api::registry::Registry;
use crate::api::routes;
use crate::config::Config;
use crate::control::ControlState;
use crate::ring::{AudioRing, EncodedRing};
use crate::web::influx_service::InfluxHistoryService;
use crate::web::peaks::PeakStorage;

#[derive(Clone)]
pub struct ApiState {
    pub peak_store: Arc<PeakStorage>,
    pub history_service: Option<Arc<InfluxHistoryService>>,
    pub ring: AudioRing,
    pub encoded_ring: EncodedRing,
    pub control_state: Arc<ControlState>,
    pub config: Arc<Config>,
    pub registry: Arc<Registry>,
    pub wav_dir: PathBuf,
    pub codec_registry: Arc<crate::codecs::registry::CodecRegistry>,
}

pub struct ApiService {
    bind_addr: SocketAddr,
}

impl ApiService {
    pub fn new(bind_addr: SocketAddr) -> Self {
        Self { bind_addr }
    }

    pub fn start(self, state: ApiState) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            let rt = match tokio::runtime::Runtime::new() {
                Ok(runtime) => runtime,
                Err(err) => {
                    error!("[api] failed to create runtime: {}", err);
                    return;
                }
            };

            if let Err(err) = rt.block_on(run_api_server(self.bind_addr, state)) {
                error!("[api] server error: {}", err);
            }
        })
    }
}

async fn run_api_server(bind_addr: SocketAddr, state: ApiState) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/api/peaks", get(routes::peaks::get_peaks))
        .route("/api/history", get(routes::peaks::get_history))
        .route("/api/status", get(routes::status::get_status))
        .route("/api/codecs", get(routes::codecs::get_codecs))
        .route("/api/devices", get(routes::devices::get_devices))
        .route(
            "/api/control",
            axum::routing::post(routes::control::post_control),
        )
        .route("/api/events", get(routes::status::events))
        .route("/api/config", get(routes::config::get_config))
        .route("/ws", get(routes::websocket::websocket_handler))
        .nest_service(
            "/",
            ServeDir::new("public").append_index_html_on_directories(true),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    info!("[api] server listening on {}", bind_addr);
    axum::serve(listener, app).await?;
    Ok(())
}
