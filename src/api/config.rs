use std::io::Read;
use std::sync::{Arc, Mutex};

use log::error;
use serde_json::json;
use tiny_http::{Header, Method, Request, Response, StatusCode};

use crate::config::{Config, ConfigPatch};

pub fn handle_config_request(req: &mut Request, config: Arc<Mutex<Config>>) {
    if req.method() != &Method::Post {
        let _ = req.respond(Response::empty(StatusCode(405)));
        return;
    }

    let mut body = String::new();
    if let Err(err) = req.as_reader().read_to_string(&mut body) {
        error!("[config] failed to read request body: {}", err);
        let _ = req.respond(Response::from_string("invalid request body").with_status_code(400));
        return;
    }

    let patch: ConfigPatch = match serde_json::from_str(&body) {
        Ok(patch) => patch,
        Err(err) => {
            error!("[config] invalid JSON payload: {}", err);
            let _ = req.respond(Response::from_string("invalid JSON payload").with_status_code(400));
            return;
        }
    };

    let mut guard = match config.lock() {
        Ok(guard) => guard,
        Err(_) => {
            let _ = req.respond(Response::from_string("config lock poisoned").with_status_code(500));
            return;
        }
    };

    if let Err(err) = guard.apply_patch(&patch) {
        error!("[config] failed to apply patch: {}", err);
        let _ = req.respond(Response::from_string(err.to_string()).with_status_code(400));
        return;
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
