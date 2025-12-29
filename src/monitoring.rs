use std::fmt::Write;
use std::sync::{Arc, Mutex};
use std::thread;

use tiny_http::{Header, Method, Response, Server, StatusCode};

use crate::core::AirliftNode;

pub fn start_monitoring_server(bind: &str, node: Arc<Mutex<AirliftNode>>) -> anyhow::Result<()> {
    let server = Server::http(bind).map_err(|e| anyhow::anyhow!(e))?;
    log::info!("[monitoring] server on {}", bind);

    thread::spawn(move || {
        for req in server.incoming_requests() {
            match (req.method(), req.url()) {
                (&Method::Get, "/health") => {
                    let running = node
                        .lock()
                        .map(|node| node.is_running())
                        .unwrap_or(false);
                    let status = if running { StatusCode(200) } else { StatusCode(503) };
                    let body = if running { "ok" } else { "not_running" };
                    let response = Response::from_string(body)
                        .with_status_code(status)
                        .with_header(Header::from_bytes("Content-Type", "text/plain").unwrap());
                    let _ = req.respond(response);
                }
                (&Method::Get, "/metrics") => {
                    let metrics = node
                        .lock()
                        .map(|node| build_metrics(&node))
                        .unwrap_or_else(|_| "# error generating metrics\n".to_string());
                    let response = Response::from_string(metrics)
                        .with_status_code(StatusCode(200))
                        .with_header(
                            Header::from_bytes("Content-Type", "text/plain; version=0.0.4")
                                .unwrap(),
                        );
                    let _ = req.respond(response);
                }
                _ => {
                    let _ = req.respond(Response::empty(StatusCode(404)));
                }
            }
        }
    });

    Ok(())
}

fn build_metrics(node: &AirliftNode) -> String {
    let mut output = String::new();
    let _ = writeln!(
        output,
        "# HELP airlift_frames_processed_total Total frames processed by producer."
    );
    let _ = writeln!(output, "# TYPE airlift_frames_processed_total counter");
    for producer in node.producers() {
        let status = producer.status();
        let _ = writeln!(
            output,
            "airlift_frames_processed_total{{producer=\"{}\"}} {}",
            escape_label_value(producer.name()),
            status.samples_processed
        );
    }

    let _ = writeln!(
        output,
        "# HELP airlift_buffer_utilization_ratio Ring buffer utilization (current_frames / capacity)."
    );
    let _ = writeln!(output, "# TYPE airlift_buffer_utilization_ratio gauge");
    let _ = writeln!(
        output,
        "# HELP airlift_buffer_frames Current frames in ring buffer."
    );
    let _ = writeln!(output, "# TYPE airlift_buffer_frames gauge");
    let _ = writeln!(
        output,
        "# HELP airlift_buffer_capacity_frames Ring buffer capacity in frames."
    );
    let _ = writeln!(output, "# TYPE airlift_buffer_capacity_frames gauge");
    let _ = writeln!(
        output,
        "# HELP airlift_buffer_latency_seconds Time span between oldest and newest frame timestamps in seconds."
    );
    let _ = writeln!(output, "# TYPE airlift_buffer_latency_seconds gauge");

    let registry = node.buffer_registry();
    for buffer_name in registry.list() {
        if let Some(buffer) = registry.get(&buffer_name) {
            let stats = buffer.stats();
            let utilization = if stats.capacity > 0 {
                stats.current_frames as f64 / stats.capacity as f64
            } else {
                0.0
            };
            let label = escape_label_value(&buffer_name);
            let _ = writeln!(
                output,
                "airlift_buffer_utilization_ratio{{buffer=\"{}\"}} {}",
                label, utilization
            );
            let _ = writeln!(
                output,
                "airlift_buffer_frames{{buffer=\"{}\"}} {}",
                label, stats.current_frames
            );
            let _ = writeln!(
                output,
                "airlift_buffer_capacity_frames{{buffer=\"{}\"}} {}",
                label, stats.capacity
            );
            if let (Some(oldest), Some(latest)) = (stats.oldest_timestamp, stats.latest_timestamp) {
                if latest >= oldest {
                    let latency = (latest - oldest) as f64 / 1_000_000_000.0;
                    let _ = writeln!(
                        output,
                        "airlift_buffer_latency_seconds{{buffer=\"{}\"}} {}",
                        label, latency
                    );
                }
            }
        }
    }

    output
}

fn escape_label_value(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\"', "\\\"")
}
