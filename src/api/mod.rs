use std::sync::{Arc, Mutex};
use std::thread;

use tiny_http::{Method, Response, Server, StatusCode};

use crate::config::Config;
use crate::core::AirliftNode;
use crate::monitoring;

pub mod catalog;
pub mod config;
pub mod control;
pub mod status;
pub mod ws;

pub fn start_api_server(
    bind: &str,
    config: Arc<Mutex<Config>>,
    node: Arc<Mutex<AirliftNode>>,
) -> anyhow::Result<()> {
    let server = Server::http(bind).map_err(|e| anyhow::anyhow!(e))?;
    log::info!("[api] server on {}", bind);

    thread::spawn(move || {
        for mut req in server.incoming_requests() {
            if req.method() == &Method::Get && req.url() == "/ws" {
                ws::handle_ws_request(req, node.clone());
                continue;
            }

            match (req.method(), req.url()) {
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
                (&Method::Post, "/api/control") => {
                    control::handle_control_request(req, config.clone(), node.clone());
                    continue;
                }
                (&Method::Get, "/api/catalog") => {
                    catalog::handle_catalog_request(req, node.clone());
                }
                _ => {
                    let _ = req.respond(Response::empty(StatusCode(404)));
                }
            }
        }
    });

    Ok(())
}
