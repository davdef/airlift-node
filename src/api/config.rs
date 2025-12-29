use std::io::Read;
use std::sync::{Arc, Mutex};
use std::thread;

use log::{error, info};
use serde_json::json;
use tiny_http::{Header, Method, Response, Server, StatusCode};

use crate::config::{Config, ConfigPatch};

pub fn start_config_api(bind: &str, config: Arc<Mutex<Config>>) -> anyhow::Result<()> {
    let server = Server::http(bind).map_err(|e| anyhow::anyhow!(e))?;
    info!("[config] API server on {}", bind);

    thread::spawn(move || {
        for mut req in server.incoming_requests() {
            if req.url() != "/api/config" {
                let _ = req.respond(Response::empty(StatusCode(404)));
                continue;
            }

            if req.method() != &Method::Post {
                let _ = req.respond(Response::empty(StatusCode(405)));
                continue;
            }

            let mut body = String::new();
            if let Err(err) = req.as_reader().read_to_string(&mut body) {
                error!("[config] failed to read request body: {}", err);
                let _ = req
                    .respond(Response::from_string("invalid request body").with_status_code(400));
                continue;
            }

            let patch: ConfigPatch = match serde_json::from_str(&body) {
                Ok(patch) => patch,
                Err(err) => {
                    error!("[config] invalid JSON payload: {}", err);
                    let _ = req.respond(
                        Response::from_string("invalid JSON payload").with_status_code(400),
                    );
                    continue;
                }
            };

            let mut guard = match config.lock() {
                Ok(guard) => guard,
                Err(_) => {
                    let _ = req.respond(
                        Response::from_string("config lock poisoned").with_status_code(500),
                    );
                    continue;
                }
            };

            if let Err(err) = guard.apply_patch(&patch) {
                error!("[config] failed to apply patch: {}", err);
                let _ = req.respond(Response::from_string(err.to_string()).with_status_code(400));
                continue;
            }

            let payload = json!({
                "status": "ok",
                "config": &*guard,
            });
            let body = payload.to_string();
            let response = Response::from_string(body)
                .with_status_code(StatusCode(200))
                .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
            let _ = req.respond(response);
        }
    });

    Ok(())
}
