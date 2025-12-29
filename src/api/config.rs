use std::io::Read;
use std::sync::{Arc, Mutex};

use log::error;
use serde_json::json;
use tiny_http::{Header, Method, Request, Response, StatusCode};

use crate::config::{Config, ConfigPatch};

pub fn handle_config_request(mut req: Request, config: Arc<Mutex<Config>>) {

let response = if req.method() != &Method::Post {
    Response::from_string("")
        .with_status_code(StatusCode(405))
} else {

        // Body lesen
        let mut body = String::new();
        if let Err(err) = req.as_reader().read_to_string(&mut body) {
            error!("[config] failed to read request body: {}", err);
            Response::from_string("invalid request body")
                .with_status_code(StatusCode(400))
        } else {
            // JSON parsen
            match serde_json::from_str::<ConfigPatch>(&body) {
                Ok(patch) => {
                    // Config locken
                    match config.lock() {
                        Ok(mut guard) => {
                            // Patch anwenden
                            match guard.apply_patch(&patch) {
                                Ok(_) => {
                                    let payload = json!({
                                        "status": "ok",
                                        "config": &*guard,
                                    });
                                    Response::from_string(payload.to_string())
                                        .with_status_code(StatusCode(200))
                                        .with_header(
                                            Header::from_bytes(
                                                "Content-Type",
                                                "application/json",
                                            )
                                            .unwrap(),
                                        )
                                }
                                Err(err) => {
                                    error!("[config] failed to apply patch: {}", err);
                                    Response::from_string(err.to_string())
                                        .with_status_code(StatusCode(400))
                                }
                            }
                        }
                        Err(_) => Response::from_string("config lock poisoned")
                            .with_status_code(StatusCode(500)),
                    }
                }
                Err(err) => {
                    error!("[config] invalid JSON payload: {}", err);
                    Response::from_string("invalid JSON payload")
                        .with_status_code(StatusCode(400))
                }
            }
        }
    };

    let _ = req.respond(response);
}
