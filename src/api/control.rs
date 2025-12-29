use serde::{Deserialize, Serialize};
use std::io::Read;
use std::sync::{Arc, Mutex};

use tiny_http::{Header, Method, Request, Response, StatusCode};

use crate::app::configurator;
use crate::config::Config;
use crate::core::AirliftNode;

#[derive(Deserialize)]
pub struct ControlRequest {
    pub action: String,
    pub target: Option<String>,
    #[serde(default)]
    pub parameters: Option<serde_json::Value>,
}

#[derive(Serialize)]
pub struct ControlResponse {
    pub ok: bool,
    pub message: String,
}

struct ControlOutcome {
    status: StatusCode,
    ok: bool,
    message: String,
}

pub fn handle_control_request(
    mut req: Request,
    config: Arc<Mutex<Config>>,
    node: Arc<Mutex<AirliftNode>>,
) {

let response = if req.method() != &Method::Post {
    Response::from_string("")
        .with_status_code(StatusCode(405))
} else {

        // Body lesen
        let mut body = String::new();
        if let Err(err) = req.as_reader().read_to_string(&mut body) {
            Response::from_string(err.to_string())
                .with_status_code(StatusCode(400))
        } else {
            // JSON parsen
            match serde_json::from_str::<ControlRequest>(&body) {
                Ok(payload) => {
                    match node.lock() {
                        Ok(mut guard) => {
                            let outcome = dispatch_control(
                                &mut guard,
                                &config,
                                &payload.action,
                                payload.target,
                                payload.parameters,
                            );

                            let body = serde_json::to_string(&ControlResponse {
                                ok: outcome.ok,
                                message: outcome.message,
                            })
                            .unwrap_or_else(|_| {
                                "{\"ok\":false,\"message\":\"serialization_error\"}".to_string()
                            });

                            Response::from_string(body)
                                .with_status_code(outcome.status)
                                .with_header(
                                    Header::from_bytes(
                                        "Content-Type",
                                        "application/json",
                                    )
                                    .unwrap(),
                                )
                        }
                        Err(_) => Response::from_string("node lock poisoned")
                            .with_status_code(StatusCode(500)),
                    }
                }
                Err(err) => Response::from_string(err.to_string())
                    .with_status_code(StatusCode(400)),
            }
        }
    };

    let _ = req.respond(response);
}

fn dispatch_control(
    node: &mut AirliftNode,
    config: &Arc<Mutex<Config>>,
    action: &str,
    target: Option<String>,
    parameters: Option<serde_json::Value>,
) -> ControlOutcome {
    match action {
        "start" => match node.start() {
            Ok(()) => ControlOutcome {
                status: StatusCode(200),
                ok: true,
                message: "node started".to_string(),
            },
            Err(err) => ControlOutcome {
                status: StatusCode(500),
                ok: false,
                message: format!("failed to start node: {}", err),
            },
        },

        "stop" => match node.stop() {
            Ok(()) => ControlOutcome {
                status: StatusCode(200),
                ok: true,
                message: "node stopped".to_string(),
            },
            Err(err) => ControlOutcome {
                status: StatusCode(500),
                ok: false,
                message: format!("failed to stop node: {}", err),
            },
        },

        "restart" => {
            if let Err(err) = node.stop() {
                return ControlOutcome {
                    status: StatusCode(500),
                    ok: false,
                    message: format!("failed to stop node: {}", err),
                };
            }
            match node.start() {
                Ok(()) => ControlOutcome {
                    status: StatusCode(200),
                    ok: true,
                    message: "node restarted".to_string(),
                },
                Err(err) => ControlOutcome {
                    status: StatusCode(500),
                    ok: false,
                    message: format!("failed to start node: {}", err),
                },
            }
        }

        "reload" | "config.reload" | "node.reload" => apply_config_from_state(node, config),

        "config.import" => apply_config_from_toml(node, config, parameters),

        "flow.start" => dispatch_flow_action(node, target, FlowAction::Start),
        "flow.stop" => dispatch_flow_action(node, target, FlowAction::Stop),
        "flow.restart" => dispatch_flow_action(node, target, FlowAction::Restart),

        _ => ControlOutcome {
            status: StatusCode(400),
            ok: false,
            message: "unknown action".to_string(),
        },
    }
}

enum FlowAction {
    Start,
    Stop,
    Restart,
}

fn dispatch_flow_action(
    node: &mut AirliftNode,
    target: Option<String>,
    action: FlowAction,
) -> ControlOutcome {
    let flow_name = match target {
        Some(name) => name,
        None => {
            return ControlOutcome {
                status: StatusCode(400),
                ok: false,
                message: "missing target".to_string(),
            }
        }
    };

    let result = match action {
        FlowAction::Start => node.start_flow_by_name(&flow_name).map(|_| "flow started"),
        FlowAction::Stop => node.stop_flow_by_name(&flow_name).map(|_| "flow stopped"),
        FlowAction::Restart => node
            .restart_flow_by_name(&flow_name)
            .map(|_| "flow restarted"),
    };

    match result {
        Ok(message) => ControlOutcome {
            status: StatusCode(200),
            ok: true,
            message: format!("{} '{}'", message, flow_name),
        },
        Err(err) => ControlOutcome {
            status: StatusCode(500),
            ok: false,
            message: format!("flow action failed: {}", err),
        },
    }
}

fn apply_config_from_state(
    node: &mut AirliftNode,
    config: &Arc<Mutex<Config>>,
) -> ControlOutcome {
    let snapshot = match config.lock() {
        Ok(guard) => guard.clone(),
        Err(_) => {
            return ControlOutcome {
                status: StatusCode(500),
                ok: false,
                message: "config lock poisoned".to_string(),
            }
        }
    };

    match configurator::apply_config(node, &snapshot) {
        Ok(()) => ControlOutcome {
            status: StatusCode(200),
            ok: true,
            message: "configuration applied".to_string(),
        },
        Err(err) => ControlOutcome {
            status: StatusCode(422),
            ok: false,
            message: format!("failed to apply configuration: {}", err),
        },
    }
}

fn apply_config_from_toml(
    node: &mut AirliftNode,
    config: &Arc<Mutex<Config>>,
    parameters: Option<serde_json::Value>,
) -> ControlOutcome {
    let toml_payload = match extract_toml(parameters) {
        Ok(payload) => payload,
        Err(message) => {
            return ControlOutcome {
                status: StatusCode(400),
                ok: false,
                message,
            }
        }
    };

    let parsed: Config = match toml::from_str(&toml_payload) {
        Ok(config) => config,
        Err(err) => {
            return ControlOutcome {
                status: StatusCode(400),
                ok: false,
                message: format!("invalid toml: {}", err),
            }
        }
    };

    if let Err(err) = configurator::apply_config(node, &parsed) {
        return ControlOutcome {
            status: StatusCode(422),
            ok: false,
            message: format!("failed to apply configuration: {}", err),
        };
    }

    match config.lock() {
        Ok(mut guard) => {
            *guard = parsed;
        }
        Err(_) => {
            return ControlOutcome {
                status: StatusCode(500),
                ok: false,
                message: "config lock poisoned".to_string(),
            }
        }
    }

    ControlOutcome {
        status: StatusCode(200),
        ok: true,
        message: "configuration imported".to_string(),
    }
}

fn extract_toml(parameters: Option<serde_json::Value>) -> Result<String, String> {
    match parameters {
        Some(serde_json::Value::String(payload)) => Ok(payload),
        Some(serde_json::Value::Object(map)) => {
            if let Some(payload) = map.get("toml").and_then(|v| v.as_str()) {
                Ok(payload.to_string())
            } else if let Some(payload) = map.get("config_toml").and_then(|v| v.as_str()) {
                Ok(payload.to_string())
            } else {
                Err("missing toml payload".to_string())
            }
        }
        Some(_) => Err("invalid parameters for toml import".to_string()),
        None => Err("missing parameters".to_string()),
    }
}

