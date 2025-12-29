use serde::Serialize;
use std::sync::{Arc, Mutex};

use tiny_http::{Header, Method, Request, Response, StatusCode};

use crate::core::timestamp::utc_ns_now;
use crate::core::{AirliftNode, ConsumerStatus, ProcessorStatus};

#[derive(Serialize)]
pub struct StatusResponse {
    pub timestamp_ns: u64,
    pub running: bool,
    pub uptime_seconds: u64,
    pub producers: Vec<ProducerStatusResponse>,
    pub flows: Vec<FlowStatusResponse>,
    pub buffers: Vec<BufferStatusResponse>,
}

#[derive(Serialize)]
pub struct ProducerStatusResponse {
    pub name: String,
    pub running: bool,
    pub connected: bool,
    pub samples_processed: u64,
    pub errors: u64,
    pub buffer: Option<BufferStatsResponse>,
}

#[derive(Serialize)]
pub struct FlowStatusResponse {
    pub name: String,
    pub running: bool,
    pub input_buffer_levels: Vec<usize>,
    pub processor_buffer_levels: Vec<usize>,
    pub output_buffer_level: usize,
    pub processors: Vec<ProcessorStatusResponse>,
    pub consumers: Vec<ConsumerStatusResponse>,
}

#[derive(Serialize)]
pub struct ProcessorStatusResponse {
    pub name: String,
    pub running: bool,
    pub processing_rate_hz: f32,
    pub latency_ms: f32,
    pub errors: u64,
}

#[derive(Serialize)]
pub struct ConsumerStatusResponse {
    pub name: String,
    pub running: bool,
    pub connected: bool,
    pub frames_processed: u64,
    pub bytes_written: u64,
    pub errors: u64,
}

#[derive(Serialize)]
pub struct BufferStatusResponse {
    pub name: String,
    pub capacity: usize,
    pub current_frames: usize,
    pub dropped_frames: u64,
    pub latest_timestamp: Option<u64>,
    pub oldest_timestamp: Option<u64>,
}

#[derive(Serialize)]
pub struct BufferStatsResponse {
    pub capacity: usize,
    pub current_frames: usize,
    pub dropped_frames: u64,
    pub latest_timestamp: Option<u64>,
    pub oldest_timestamp: Option<u64>,
}

pub fn handle_status_request(req: &mut Request, node: Arc<Mutex<AirliftNode>>) {
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
            ProducerStatusResponse {
                name: producer.name().to_string(),
                running: status.running,
                connected: status.connected,
                samples_processed: status.samples_processed,
                errors: status.errors,
                buffer: status.buffer_stats.as_ref().map(map_buffer_stats),
            }
        })
        .collect::<Vec<_>>();

    let flows = node
        .flows()
        .iter()
        .map(|flow| {
            let status = flow.status();
            let processor_names = flow.processor_names();
            let consumer_names = flow.consumer_names();

            FlowStatusResponse {
                name: flow.name.clone(),
                running: status.running,
                input_buffer_levels: status.input_buffer_levels,
                processor_buffer_levels: status.processor_buffer_levels,
                output_buffer_level: status.output_buffer_level,
                processors: map_processor_statuses(&processor_names, status.processor_status),
                consumers: map_consumer_statuses(&consumer_names, status.consumer_status),
            }
        })
        .collect::<Vec<_>>();

    let registry = node.buffer_registry();
    let buffers = registry
        .list()
        .into_iter()
        .filter_map(|name| {
            registry.get(&name).map(|buffer| {
                let stats = buffer.stats();
                BufferStatusResponse {
                    name,
                    capacity: stats.capacity,
                    current_frames: stats.current_frames,
                    dropped_frames: stats.dropped_frames,
                    latest_timestamp: stats.latest_timestamp,
                    oldest_timestamp: stats.oldest_timestamp,
                }
            })
        })
        .collect::<Vec<_>>();

    StatusResponse {
        timestamp_ns: utc_ns_now(),
        running: node_status.running,
        uptime_seconds: node_status.uptime_seconds,
        producers,
        flows,
        buffers,
    }
}

fn map_buffer_stats(stats: &crate::core::RingBufferStats) -> BufferStatsResponse {
    BufferStatsResponse {
        capacity: stats.capacity,
        current_frames: stats.current_frames,
        dropped_frames: stats.dropped_frames,
        latest_timestamp: stats.latest_timestamp,
        oldest_timestamp: stats.oldest_timestamp,
    }
}

fn map_processor_statuses(
    names: &[String],
    statuses: Vec<ProcessorStatus>,
) -> Vec<ProcessorStatusResponse> {
    names
        .iter()
        .cloned()
        .zip(statuses.into_iter())
        .map(|(name, status)| ProcessorStatusResponse {
            name,
            running: status.running,
            processing_rate_hz: status.processing_rate_hz,
            latency_ms: status.latency_ms,
            errors: status.errors,
        })
        .collect()
}

fn map_consumer_statuses(
    names: &[String],
    statuses: Vec<ConsumerStatus>,
) -> Vec<ConsumerStatusResponse> {
    names
        .iter()
        .cloned()
        .zip(statuses.into_iter())
        .map(|(name, status)| ConsumerStatusResponse {
            name,
            running: status.running,
            connected: status.connected,
            frames_processed: status.frames_processed,
            bytes_written: status.bytes_written,
            errors: status.errors,
        })
        .collect()
}
