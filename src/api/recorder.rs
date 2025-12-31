use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use serde::Serialize;
use tiny_http::{Header, Method, Request, Response, StatusCode};

use crate::core::lock::lock_mutex;
use crate::core::{AirliftNode, Flow};
use crate::producers::ws::{WsHandle, WsProducer};

static RECORDER_COUNTER: AtomicU64 = AtomicU64::new(1);
static RECORDER_HANDLES: OnceLock<Mutex<HashMap<String, WsHandle>>> = OnceLock::new();

fn recorder_registry() -> &'static Mutex<HashMap<String, WsHandle>> {
    RECORDER_HANDLES.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Serialize)]
struct RecorderStartResponse {
    producer_id: String,
}

pub fn handle_recorder_start(
    req: Request,
    node: Arc<Mutex<AirliftNode>>,
) {
    let response = if req.method() != &Method::Post {
        Response::from_string("").with_status_code(StatusCode(405))
    } else {
        let producer_id = format!(
            "recorder-{}",
            RECORDER_COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let (producer, handle) = WsProducer::new(&producer_id);

        match node.lock() {
            Ok(mut guard) => match guard.add_producer(Box::new(producer)) {
                Ok(()) => {
                    let buffer_name = format!("producer:{}", producer_id);
                    let flow_name = producer_id.clone();
                    if guard.flow_index_by_name(&flow_name).is_none() {
                        guard.add_flow(Flow::new(&flow_name));
                    }

                    if let Some(flow_index) = guard.flow_index_by_name(&flow_name) {
                        if let Err(err) = guard.connect_flow_input(flow_index, &buffer_name) {
                            log::warn!(
                                "Failed to connect recorder '{}' to flow '{}': {}",
                                producer_id,
                                flow_name,
                                err
                            );
                        }

                        if let Err(err) = guard.start_flow_by_name(&flow_name) {
                            log::warn!(
                                "Failed to start recorder flow '{}': {}",
                                flow_name,
                                err
                            );
                        }
                    } else {
                        log::warn!(
                            "Recorder flow '{}' not found; recorder '{}' not connected",
                            flow_name,
                            producer_id
                        );
                    }

                    let mut registry =
                        lock_mutex(recorder_registry(), "api.recorder.register");
                    registry.insert(producer_id.clone(), handle);

                    let payload = serde_json::to_string(&RecorderStartResponse {
                        producer_id,
                    })
                    .unwrap_or_else(|_| {
                        "{\"producer_id\":\"serialization_error\"}".to_string()
                    });
                    Response::from_string(payload)
                        .with_status_code(StatusCode(200))
                        .with_header(
                            Header::from_bytes("Content-Type", "application/json")
                                .unwrap(),
                        )
                }
                Err(err) => Response::from_string(err.to_string())
                    .with_status_code(StatusCode(500)),
            },
            Err(_) => Response::from_string("node lock poisoned")
                .with_status_code(StatusCode(500)),
        }
    };

    let _ = req.respond(response);
}

pub fn get_recorder_handle(producer_id: &str) -> Option<WsHandle> {
    let registry = lock_mutex(recorder_registry(), "api.recorder.lookup");
    registry.get(producer_id).cloned()
}
