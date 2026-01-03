use std::sync::{Arc, Mutex};
use std::thread;

use tiny_http::{Method, Response, Server, StatusCode};

use crate::config::Config;
use crate::core::AirliftNode;
use crate::monitoring;

pub mod catalog;
pub mod config;
pub mod control;
pub mod peaks;
pub mod recorder;
pub mod status;
pub mod ws;

pub fn start_api_server(
    bind: &str,
    config: Arc<Mutex<Config>>,
    node: Arc<Mutex<AirliftNode>>,
) -> anyhow::Result<()> {
    let server = Server::http(bind).map_err(|e| anyhow::anyhow!(e))?;
    log::info!("[api] server on {}", bind);

    let peak_history = peaks::register_peak_history(node.clone());

    thread::spawn(move || {
        for mut req in server.incoming_requests() {
            let url = req.url().to_string();
            let (path, query) = url.split_once('?').unwrap_or((&url, ""));

            if req.method() == &Method::Get && path.starts_with("/ws/recorder/") {
                let producer_id = path
                    .trim_start_matches("/ws/recorder/")
                    .to_string();
                ws::handle_recorder_ws_request(req, node.clone(), producer_id);
                continue;
            }

            if req.method() == &Method::Get && path == "/ws" {
                ws::handle_ws_request(req, node.clone());
                continue;
            }

            match (req.method(), path) {
                (&Method::Get, "/health") => {
                    monitoring::handle_health_request(req, node.clone());
                    continue;
                }
                (&Method::Get, "/metrics") => {
                    monitoring::handle_metrics_request(req, node.clone());
                    continue;
                }
                (&Method::Post, "/api/config") => {
                    config::handle_config_request(req, config.clone());
                    continue;
                }
                (&Method::Get, "/api/status") => {
                    status::handle_status_request(req, node.clone());
                    continue;
                }
                (&Method::Get, "/api/peaks") => {
                    peaks::handle_peaks_request(req, peak_history.clone());
                    continue;
                }
                (&Method::Get, "/api/history") => {
                    peaks::handle_history_request(
                        req,
                        peak_history.clone(),
                        if query.is_empty() { None } else { Some(query) },
                    );
                    continue;
                }
                (&Method::Post, "/api/control") => {
                    control::handle_control_request(req, config.clone(), node.clone());
                    continue;
                }
                (&Method::Post, "/api/recorder/start") => {
                    recorder::handle_recorder_start(req, node.clone());
                    continue;
                }
                (&Method::Get, "/api/catalog") => {
                    catalog::handle_catalog_request(req, node.clone());
                }
    (&Method::Post, "/api/recorder/start") => {
        recorder::handle_recorder_start(req, node.clone());
    }
    
    // Neue Route für Stop
    _ if req.method() == &Method::Post && path.starts_with("/api/recorder/stop/") => {
        recorder::handle_recorder_stop(req, node.clone());
    }

_ if req.method() == &Method::Get && path.starts_with("/ws/echo/") => {
    let session_id = path.trim_start_matches("/ws/echo/").to_string();
    println!("DEBUG: Echo WS request for session: {}", session_id); // Log hinzufügen
    ws::handle_echo_ws_request(req, node.clone(), session_id);
}

                _ => {
                    let _ = req.respond(Response::empty(StatusCode(404)));
                }
            }
        }
    });

    Ok(())
}
