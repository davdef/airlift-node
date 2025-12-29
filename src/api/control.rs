use serde::{Deserialize, Serialize};
use std::io::Read;
use std::sync::{Arc, Mutex};

use tiny_http::{Header, Method, Request, Response, StatusCode};

use crate::core::AirliftNode;

#[derive(Deserialize)]
pub struct ControlRequest {
    pub action: String,
    pub target: Option<String>,
}

#[derive(Serialize)]
pub struct ControlResponse {
    pub ok: bool,
    pub message: String,
}

pub fn handle_control_request(req: &mut Request, node: Arc<Mutex<AirliftNode>>) {
    if req.method() != &Method::Post {
        let _ = req.respond(Response::empty(StatusCode(405)));
        return;
    }

    let mut body = String::new();
    if let Err(err) = req.as_reader().read_to_string(&mut body) {
        let _ = req.respond(Response::from_string(err.to_string()).with_status_code(400));
        return;
    }

    let payload: ControlRequest = match serde_json::from_str(&body) {
        Ok(payload) => payload,
        Err(err) => {
            let _ = req.respond(Response::from_string(err.to_string()).with_status_code(400));
            return;
        }
    };

    let response = match node.lock() {
        Ok(mut guard) => {
            let (ok, message) = dispatch_control(&mut guard, &payload.action, payload.target);
            let body = serde_json::to_string(&ControlResponse { ok, message }).unwrap_or_else(|_| {
                "{\"ok\":false,\"message\":\"serialization_error\"}".to_string()
            });
            Response::from_string(body)
                .with_status_code(StatusCode(200))
                .with_header(Header::from_bytes("Content-Type", "application/json").unwrap())
        }
        Err(_) => Response::from_string("node lock poisoned").with_status_code(StatusCode(500)),
    };

    let _ = req.respond(response);
}

fn dispatch_control(
    node: &mut AirliftNode,
    action: &str,
    target: Option<String>,
) -> (bool, String) {
    match action {
        "start" => match node.start() {
            Ok(()) => (true, "node started".to_string()),
            Err(err) => (false, format!("failed to start node: {}", err)),
        },
        "stop" => match node.stop() {
            Ok(()) => (true, "node stopped".to_string()),
            Err(err) => (false, format!("failed to stop node: {}", err)),
        },
        "restart" => {
            if let Err(err) = node.stop() {
                return (false, format!("failed to stop node: {}", err));
            }
            match node.start() {
                Ok(()) => (true, "node restarted".to_string()),
                Err(err) => (false, format!("failed to start node: {}", err)),
            }
        }
        "flow.start" => {
            let flow_name = match target {
                Some(name) => name,
                None => return (false, "missing target".to_string()),
            };
            match node.flows.iter_mut().find(|flow| flow.name == flow_name) {
                Some(flow) => match flow.start() {
                    Ok(()) => (true, format!("flow '{}' started", flow_name)),
                    Err(err) => (false, format!("failed to start flow: {}", err)),
                },
                None => (false, "flow not found".to_string()),
            }
        }
        "flow.stop" => {
            let flow_name = match target {
                Some(name) => name,
                None => return (false, "missing target".to_string()),
            };
            match node.flows.iter_mut().find(|flow| flow.name == flow_name) {
                Some(flow) => match flow.stop() {
                    Ok(()) => (true, format!("flow '{}' stopped", flow_name)),
                    Err(err) => (false, format!("failed to stop flow: {}", err)),
                },
                None => (false, "flow not found".to_string()),
            }
        }
        _ => (false, "unknown action".to_string()),
    }
}
