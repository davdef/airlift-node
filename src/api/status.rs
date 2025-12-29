use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use tiny_http::{Header, Method, Request, Response, StatusCode};

use crate::core::AirliftNode;

#[derive(Serialize)]
pub struct StatusResponse {
    pub running: bool,
    pub uptime_seconds: u64,
    pub producers: Vec<ProducerInfo>,
    pub flows: Vec<FlowInfo>,
    pub ringbuffer: RingBufferInfo,
    pub modules: Vec<ModuleInfo>,
    pub inactive_modules: Vec<InactiveModule>,
    pub configuration_issues: Vec<ConfigurationIssue>,
    pub timestamp_ms: u64,
}

#[derive(Serialize)]
pub struct ProducerInfo {
    pub name: String,
    pub running: bool,
    pub connected: bool,
    pub samples_processed: u64,
    pub errors: u64,
}

#[derive(Serialize)]
pub struct FlowInfo {
    pub name: String,
    pub running: bool,
    pub input_buffer_levels: Vec<usize>,
    pub processor_buffer_levels: Vec<usize>,
    pub output_buffer_level: usize,
}

#[derive(Serialize)]
pub struct RingBufferInfo {
    pub fill: u64,
    pub capacity: u64,
}

#[derive(Serialize)]
pub struct ModuleInfo {
    pub id: String,
    pub label: String,
    pub module_type: String,
    pub runtime: ModuleRuntime,
    pub controls: Vec<ModuleControl>,
}

#[derive(Serialize)]
pub struct ModuleRuntime {
    pub enabled: bool,
    pub running: bool,
    pub connected: Option<bool>,
    pub counters: ModuleCounters,
    pub last_activity_ms: u64,
}

#[derive(Serialize)]
pub struct ModuleCounters {
    pub rx: u64,
    pub tx: u64,
    pub errors: u64,
}

#[derive(Serialize)]
pub struct ModuleControl {
    pub action: String,
    pub label: String,
    pub enabled: bool,
    pub reason: Option<String>,
}

#[derive(Serialize)]
pub struct InactiveModule {
    pub id: String,
    pub label: String,
    pub module_type: String,
    pub reason: String,
}

#[derive(Serialize)]
pub struct ConfigurationIssue {
    pub key: String,
    pub message: String,
}

pub fn handle_status_request(mut req: Request, node: Arc<Mutex<AirliftNode>>) {
    if req.method() != &Method::Get {
        let _ = req.respond(Response::empty(StatusCode(405)));
        return;
    }

    let response = match node.lock() {
        Ok(guard) => {
            let status = build_status(&guard);
            let body = serde_json::to_string(&status).unwrap_or_else(|_| "{}".to_string());
            Response::from_string(body)
                .with_status_code(StatusCode(200))
                .with_header(Header::from_bytes("Content-Type", "application/json").unwrap())
        }
        Err(_) => Response::from_string("node lock poisoned").with_status_code(StatusCode(500)),
    };

    let _ = req.respond(response);
}

fn build_status(node: &AirliftNode) -> StatusResponse {
    let node_status = node.status();

    let producers = node
        .producers()
        .iter()
        .map(|producer| {
            let status = producer.status();
            ProducerInfo {
                name: producer.name().to_string(),
                running: status.running,
                connected: status.connected,
                samples_processed: status.samples_processed,
                errors: status.errors,
            }
        })
        .collect::<Vec<_>>();

    let flows = node
        .flows()
        .iter()
        .map(|flow| {
            let status = flow.status();
            FlowInfo {
                name: flow.name.clone(),
                running: status.running,
                input_buffer_levels: status.input_buffer_levels,
                processor_buffer_levels: status.processor_buffer_levels,
                output_buffer_level: status.output_buffer_level,
            }
        })
        .collect::<Vec<_>>();

    let registry = node.buffer_registry();
    let mut ringbuffer_fill = 0_u64;
    let mut ringbuffer_capacity = 0_u64;
    for name in registry.list() {
        if let Some(buffer) = registry.get(&name) {
            let stats = buffer.stats();
            ringbuffer_fill += stats.current_frames as u64;
            ringbuffer_capacity += stats.capacity as u64;
        }
    }

    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0);

    StatusResponse {
        running: node_status.running,
        uptime_seconds: node_status.uptime_seconds,
        producers,
        flows,
        ringbuffer: RingBufferInfo {
            fill: ringbuffer_fill,
            capacity: ringbuffer_capacity,
        },
        modules: Vec::new(),
        inactive_modules: Vec::new(),
        configuration_issues: Vec::new(),
        timestamp_ms,
    }
}
