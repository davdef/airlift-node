use axum::{
    Json,
    extract::State,
    response::sse::{Event, Sse},
};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::sync::Arc;
use tokio::time::{Duration, interval};
use tokio_stream::{StreamExt, wrappers::IntervalStream};

use crate::api::{ApiState, Registry, ServiceDescriptor};
use crate::codecs::{CodecInfo, ContainerKind};
use crate::codecs::registry::{CodecInstanceSnapshot, CodecRegistry};
use crate::config::Config;
use crate::control::{ControlState, CountersSnapshot, ModuleSnapshot, now_ms};
use crate::ring::RingStats;

#[derive(Serialize, Clone)]
pub struct RingStatus {
    pub capacity: usize,
    pub head_seq: u64,
    pub next_seq: u64,
    pub fill: u64,
    pub head_index: u64,
    pub tail_index: u64,
    pub fill_ratio: f64,
    pub arc_replacements: u64,
}

#[derive(Serialize, Clone)]
pub struct ConfigRequirement {
    pub key: String,
    pub message: String,
}

#[derive(Serialize, Clone)]
pub struct ControlInfo {
    pub action: String,
    pub label: String,
    pub enabled: bool,
    pub reason: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct ModuleDetails {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buffer: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct ModuleInfo {
    pub id: String,
    pub label: String,
    #[serde(rename = "type")]
    pub module_type: String,
    pub runtime: ModuleSnapshot,
    pub controls: Vec<ControlInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codec_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codec_info: Option<CodecInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<ModuleDetails>,
}

#[derive(Serialize, Clone)]
pub struct InactiveModule {
    pub id: String,
    pub label: String,
    #[serde(rename = "type")]
    pub module_type: String,
    pub reason: String,
    pub can_activate: bool,
    pub activate_action: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub kind: String,
}

#[derive(Serialize, Clone)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
}

#[derive(Serialize, Clone)]
pub struct GraphStatus {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

#[derive(Serialize, Clone)]
pub struct FileOutStatus {
    pub enabled: bool,
    pub path: String,
    pub format: String,
    pub retention_days: u64,
    pub current_files: Vec<String>,
    pub controls: Vec<ControlInfo>,
}

#[derive(Serialize, Clone)]
pub struct StatusResponse {
    pub timestamp_ms: u64,
    pub ring: RingStatus,
    pub monitoring_available: bool,
    pub configuration_required: bool,
    pub configuration_issues: Vec<ConfigRequirement>,
    pub modules: Vec<ModuleInfo>,
    pub inactive_modules: Vec<InactiveModule>,
    pub graph: GraphStatus,
    pub file_out: FileOutStatus,
}

pub async fn get_status(State(state): State<ApiState>) -> Json<StatusResponse> {
    Json(build_status(
        &state.control_state,
        &state.ring.stats(),
        &state.config,
        &state.registry,
        &state.codec_registry,
    ))
}

pub async fn events(
    State(state): State<ApiState>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let control_state = state.control_state.clone();
    let ring = state.ring.clone();
    let config = state.config.clone();
    let registry = state.registry.clone();
    let codec_registry = state.codec_registry.clone();

    let stream = IntervalStream::new(interval(Duration::from_secs(1))).map(move |_| {
        let status = build_status(
            &control_state,
            &ring.stats(),
            &config,
            &registry,
            &codec_registry,
        );
        let data = serde_json::to_string(&status).unwrap_or_else(|_| "{}".to_string());
        Ok(Event::default().event("status").data(data))
    });

    Sse::new(stream)
}

fn build_status(
    control_state: &Arc<ControlState>,
    ring_stats: &RingStats,
    config: &Config,
    registry: &Registry,
    codec_registry: &CodecRegistry,
) -> StatusResponse {
    let fill = ring_stats
        .head_seq
        .saturating_sub(ring_stats.next_seq.wrapping_sub(1));
    let capacity = ring_stats.capacity.max(1) as u64;
    let head_index = ring_stats.head_seq % capacity;
    let tail_index = ring_stats.next_seq % capacity;
    let fill_ratio = (fill as f64 / capacity as f64).clamp(0.0, 1.0);

    let mut modules = Vec::new();
    let mut inactive_modules = Vec::new();

    let configuration_issues = build_config_requirements(config);

    let ring_id = graph_ringbuffer_id(config);

    let srt_in = control_state.srt_in.module.snapshot();
    let srt_out = control_state.srt_out.module.snapshot();
    let alsa_in = control_state.alsa_in.snapshot();
    let icecast_in = control_state.icecast_in.snapshot();
    let icecast_out = control_state.icecast_out.snapshot();
    let file_in = control_state.file_in.snapshot();
    let file_out = control_state.file_out.snapshot();
    let ring_module = control_state.ring.snapshot();

    let codec_snapshots = codec_registry.snapshots();
    let codec_map: HashMap<String, CodecInstanceSnapshot> = codec_snapshots
        .iter()
        .cloned()
        .map(|snapshot| (snapshot.id.clone(), snapshot))
        .collect();

    let srt_in_controls = build_controls(
        "srt_in",
        &srt_in,
        "Nicht unterstützt",
        Some(ControlInfo {
            action: "srt_in.force_disconnect".to_string(),
            label: "Disconnect".to_string(),
            enabled: srt_in.connected,
            reason: if srt_in.connected {
                None
            } else {
                Some("Keine Verbindung aktiv".to_string())
            },
        }),
    );

    let srt_out_controls = build_controls(
        "srt_out",
        &srt_out,
        "Nicht unterstützt",
        Some(ControlInfo {
            action: "srt_out.reconnect".to_string(),
            label: "Reconnect".to_string(),
            enabled: srt_out.running,
            reason: if srt_out.running {
                None
            } else {
                Some("Modul nicht gestartet".to_string())
            },
        }),
    );

    let file_out_controls = build_controls("file_out", &file_out, "Vorbereitet", None);

    if let Some(ring_id) = ring_id {
        add_module_if_active(
            &mut modules,
            ring_id,
            "Ringbuffer",
            "buffer",
            ring_module.clone(),
            build_controls("ring", &ring_module, "Nicht unterstützt", None),
            None,
            None,
            None,
        );
    }

    for (id, input) in config.inputs.iter() {
        let (snapshot, label, details, controls) = match input.input_type.as_str() {
            "srt" => (srt_in.clone(), "SRT-IN", None, srt_in_controls.clone()),
            "icecast" | "http_stream" => (
                icecast_in.clone(),
                "Icecast-IN",
                Some(ModuleDetails {
                    input_type: Some(input.input_type.clone()),
                    url: input.url.clone(),
                    buffer: Some(input.buffer.clone()),
                }),
                build_controls("icecast_in", &icecast_in, "Nicht unterstützt", None),
            ),
            "alsa" => (
                alsa_in.clone(),
                "ALSA-IN",
                None,
                build_controls("alsa_in", &alsa_in, "Nicht unterstützt", None),
            ),
            "file_in" => (
                file_in.clone(),
                "File-In",
                Some(ModuleDetails {
                    input_type: Some(input.input_type.clone()),
                    url: input.path.clone(),
                    buffer: Some(input.buffer.clone()),
                }),
                build_controls("file_in", &file_in, "Nicht unterstützt", None),
            ),
            _ => continue,
        };

        add_module_if_active(
            &mut modules,
            id,
            label,
            "input",
            snapshot,
            controls,
            None,
            None,
            details,
        );
    }

    for (id, output) in config.outputs.iter() {
        let (snapshot, label, controls) = match output.output_type.as_str() {
            "srt_out" => (srt_out.clone(), "SRT-OUT", srt_out_controls.clone()),
            "icecast_out" => (
                icecast_out.clone(),
                "Icecast-Out",
                build_controls("icecast_out", &icecast_out, "Nicht unterstützt", None),
            ),
            "file_out" => (file_out.clone(), "File-Out", file_out_controls.clone()),
            _ => (
                static_snapshot(output.enabled, output.enabled),
                output.output_type.as_str(),
                build_controls(&output.output_type, &ring_module, "Nicht unterstützt", None),
            ),
        };

        let codec_id = output.codec_id.clone();
        let codec_info = codec_id
            .as_deref()
            .and_then(|id| codec_registry.get_info(id).ok());

        add_module_if_active(
            &mut modules,
            id,
            label,
            "output",
            snapshot,
            controls,
            codec_id,
            codec_info,
            None,
        );
    }

    for snapshot in codec_snapshots.iter() {
        if snapshot.runtime_state.enabled && snapshot.runtime_state.running {
            modules.push(ModuleInfo {
                id: snapshot.id.clone(),
                label: format!("Codec {}", snapshot.id),
                module_type: "codec".to_string(),
                runtime: snapshot.runtime_state.clone(),
                controls: build_controls(
                    &snapshot.id,
                    &snapshot.runtime_state,
                    "Nicht unterstützt",
                    None,
                ),
                codec_id: None,
                codec_info: None,
                details: None,
            });
        }
    }

    for (id, input) in config.inputs.iter() {
        let (snapshot, label) = match input.input_type.as_str() {
            "srt" => (srt_in.clone(), "SRT-IN"),
            "icecast" | "http_stream" => (icecast_in.clone(), "Icecast-IN"),
            "alsa" => (alsa_in.clone(), "ALSA-IN"),
            "file_in" => (file_in.clone(), "File-In"),
            _ => continue,
        };
        add_inactive_module(
            &mut inactive_modules,
            id,
            label,
            "input",
            &snapshot,
            Some(input.enabled),
        );
    }

    for (id, output) in config.outputs.iter() {
        let (snapshot, label) = match output.output_type.as_str() {
            "srt_out" => (srt_out.clone(), "SRT-OUT"),
            "icecast_out" => (icecast_out.clone(), "Icecast-Out"),
            "file_out" => (file_out.clone(), "File-Out"),
            _ => (
                static_snapshot(output.enabled, output.enabled),
                output.output_type.as_str(),
            ),
        };

        add_inactive_module(
            &mut inactive_modules,
            id,
            label,
            "output",
            &snapshot,
            Some(output.enabled),
        );
    }

    let graph = build_graph(
        &srt_in,
        &icecast_in,
        &alsa_in,
        &file_in,
        &srt_out,
        &icecast_out,
        &file_out,
        &ring_module,
        config,
        registry,
        &codec_map,
    );

    let file_out_spec = find_file_out_signal(config);
    let file_out_status = build_file_out_status(file_out_spec, file_out_controls, codec_registry);
    let monitoring_available = config
        .services
        .values()
        .any(|service| service.enabled && service.service_type == "monitor");

    StatusResponse {
        timestamp_ms: now_ms(),
        ring: RingStatus {
            capacity: ring_stats.capacity,
            head_seq: ring_stats.head_seq,
            next_seq: ring_stats.next_seq,
            fill,
            head_index,
            tail_index,
            fill_ratio,
            arc_replacements: ring_stats.arc_replacements,
        },
        monitoring_available,
        configuration_required: !configuration_issues.is_empty(),
        configuration_issues,
        modules,
        inactive_modules,
        graph,
        file_out: file_out_status,
    }
}

fn add_module_if_active(
    modules: &mut Vec<ModuleInfo>,
    id: &str,
    label: &str,
    module_type: &str,
    snapshot: ModuleSnapshot,
    controls: Vec<ControlInfo>,
    codec_id: Option<String>,
    codec_info: Option<CodecInfo>,
    details: Option<ModuleDetails>,
) {
    if snapshot.enabled && snapshot.running {
        modules.push(ModuleInfo {
            id: id.to_string(),
            label: label.to_string(),
            module_type: module_type.to_string(),
            runtime: snapshot,
            controls,
            codec_id,
            codec_info,
            details,
        });
    }
}

fn add_inactive_module(
    inactive_modules: &mut Vec<InactiveModule>,
    id: &str,
    label: &str,
    module_type: &str,
    snapshot: &ModuleSnapshot,
    config_enabled: Option<bool>,
) {
    if snapshot.enabled && snapshot.running {
        return;
    }
    let reason = match config_enabled {
        None => "Nicht konfiguriert".to_string(),
        Some(false) => "Deaktiviert in Konfiguration".to_string(),
        Some(true) => {
            if !snapshot.enabled {
                "Deaktiviert".to_string()
            } else if !snapshot.running {
                "Nicht gestartet".to_string()
            } else {
                "Inaktiv".to_string()
            }
        }
    };

    inactive_modules.push(InactiveModule {
        id: id.to_string(),
        label: label.to_string(),
        module_type: module_type.to_string(),
        reason,
        can_activate: false,
        activate_action: None,
    });
}

fn build_graph(
    srt_in: &ModuleSnapshot,
    icecast_in: &ModuleSnapshot,
    alsa_in: &ModuleSnapshot,
    file_in: &ModuleSnapshot,
    srt_out: &ModuleSnapshot,
    icecast_out: &ModuleSnapshot,
    file_out: &ModuleSnapshot,
    ring: &ModuleSnapshot,
    config: &Config,
    registry: &Registry,
    codec_map: &HashMap<String, CodecInstanceSnapshot>,
) -> GraphStatus {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let ring_id = graph_ringbuffer_id(config);

    let ring_active = ring_id.is_some() && ring.enabled && ring.running;
    if let Some(ring_id) = ring_id {
        if ring_active {
            nodes.push(GraphNode {
                id: ring_id.to_string(),
                label: "Ringbuffer".to_string(),
                kind: "buffer".to_string(),
            });
        }

        if ring_active {
            for service in registry.list_services() {
                if service_consumes_ring(&service) {
                    let service_id = format!("service:{}", service.id);
                    if !nodes.iter().any(|node| node.id == service_id) {
                        nodes.push(GraphNode {
                            id: service_id.clone(),
                            label: humanize_service_label(&service),
                            kind: "service".to_string(),
                        });
                        edges.push(GraphEdge {
                            from: ring_id.to_string(),
                            to: service_id,
                        });
                    }
                }
            }
        }
    }

    for (id, input) in config.inputs.iter() {
        let (snapshot, label) = match input.input_type.as_str() {
            "srt" => (srt_in, "SRT-IN"),
            "icecast" | "http_stream" => (icecast_in, "Icecast-IN"),
            "alsa" => (alsa_in, "ALSA-IN"),
            "file_in" => (file_in, "File-In"),
            _ => continue,
        };
        if snapshot.enabled && snapshot.running {
            nodes.push(GraphNode {
                id: id.to_string(),
                label: label.to_string(),
                kind: "input".to_string(),
            });
            if ring_active {
                if let Some(ring_id) = ring_id {
                    edges.push(GraphEdge {
                        from: id.to_string(),
                        to: ring_id.to_string(),
                    });
                }
            }
        }
    }

    for (id, output) in config.outputs.iter() {
        let (snapshot, label) = match output.output_type.as_str() {
            "srt_out" => (srt_out, "SRT-OUT"),
            "icecast_out" => (icecast_out, "Icecast-Out"),
            "file_out" => (file_out, "File-Out"),
            _ => {
                if !output.enabled {
                    continue;
                }
                add_output_node(
                    &mut nodes,
                    &mut edges,
                    ring_id,
                    id,
                    output.output_type.as_str(),
                    ring_active,
                    output.codec_id.clone(),
                    codec_map,
                );
                continue;
            }
        };

        add_output_to_graph(
            &mut nodes,
            &mut edges,
            ring_id,
            id,
            label,
            snapshot,
            ring_active,
            output.codec_id.clone(),
            codec_map,
        );
    }

    GraphStatus { nodes, edges }
}

fn service_consumes_ring(service: &ServiceDescriptor) -> bool {
    matches!(
        service.service_type.as_str(),
        "audio_http"
            | "monitor"
            | "monitoring"
            | "peak_analyzer"
            | "influx_out"
            | "broadcast_http"
            | "file_out"
    )
}

fn humanize_service_label(service: &ServiceDescriptor) -> String {
    match service.service_type.as_str() {
        "audio_http" => "Audio HTTP".to_string(),
        "monitor" => "Monitor".to_string(),
        "monitoring" => "Monitoring".to_string(),
        "file_out" => "File-Out".to_string(),
        _ => service.service_type.replace('_', " ").to_string(),
    }
}

fn add_output_to_graph(
    nodes: &mut Vec<GraphNode>,
    edges: &mut Vec<GraphEdge>,
    ring_id: Option<&str>,
    id: &str,
    label: &str,
    snapshot: &ModuleSnapshot,
    ring_active: bool,
    codec_id: Option<String>,
    codec_map: &HashMap<String, CodecInstanceSnapshot>,
) {
    if !(snapshot.enabled && snapshot.running) {
        return;
    }

    nodes.push(GraphNode {
        id: id.to_string(),
        label: label.to_string(),
        kind: "output".to_string(),
    });

    if ring_active {
        let Some(ring_id) = ring_id else {
            return;
        };
        let Some(codec_id) = codec_id else {
            edges.push(GraphEdge {
                from: ring_id.to_string(),
                to: id.to_string(),
            });
            return;
        };

        let codec_snapshot = codec_map.get(&codec_id);
        let codec_active = codec_snapshot
            .map(|snapshot| snapshot.runtime_state.enabled && snapshot.runtime_state.running)
            .unwrap_or(false);

        if codec_active {
            let codec_node_id = format!("codec:{}", codec_id);
            if !nodes.iter().any(|node| node.id == codec_node_id) {
                nodes.push(GraphNode {
                    id: codec_node_id.clone(),
                    label: format!("Codec {}", codec_id),
                    kind: "processing".to_string(),
                });
            }
            edges.push(GraphEdge {
                from: ring_id.to_string(),
                to: codec_node_id.clone(),
            });
            edges.push(GraphEdge {
                from: codec_node_id,
                to: id.to_string(),
            });
        } else {
            edges.push(GraphEdge {
                from: ring_id.to_string(),
                to: id.to_string(),
            });
        }
    }
}

fn add_output_node(
    nodes: &mut Vec<GraphNode>,
    edges: &mut Vec<GraphEdge>,
    ring_id: Option<&str>,
    id: &str,
    label: &str,
    ring_active: bool,
    codec_id: Option<String>,
    codec_map: &HashMap<String, CodecInstanceSnapshot>,
) {
    nodes.push(GraphNode {
        id: id.to_string(),
        label: label.to_string(),
        kind: "output".to_string(),
    });

    if ring_active {
        let Some(ring_id) = ring_id else {
            return;
        };
        let Some(codec_id) = codec_id else {
            edges.push(GraphEdge {
                from: ring_id.to_string(),
                to: id.to_string(),
            });
            return;
        };

        let codec_snapshot = codec_map.get(&codec_id);
        let codec_active = codec_snapshot
            .map(|snapshot| snapshot.runtime_state.enabled && snapshot.runtime_state.running)
            .unwrap_or(false);

        if codec_active {
            let codec_node_id = format!("codec:{}", codec_id);
            if !nodes.iter().any(|node| node.id == codec_node_id) {
                nodes.push(GraphNode {
                    id: codec_node_id.clone(),
                    label: format!("Codec {}", codec_id),
                    kind: "processing".to_string(),
                });
            }
            edges.push(GraphEdge {
                from: ring_id.to_string(),
                to: codec_node_id.clone(),
            });
            edges.push(GraphEdge {
                from: codec_node_id,
                to: id.to_string(),
            });
        } else {
            edges.push(GraphEdge {
                from: ring_id.to_string(),
                to: id.to_string(),
            });
        }
    }
}

fn build_controls(
    _id: &str,
    _snapshot: &ModuleSnapshot,
    base_reason: &str,
    extra: Option<ControlInfo>,
) -> Vec<ControlInfo> {
    let mut controls = vec![
        ControlInfo {
            action: "module.start".to_string(),
            label: "Start".to_string(),
            enabled: false,
            reason: Some(base_reason.to_string()),
        },
        ControlInfo {
            action: "module.stop".to_string(),
            label: "Stop".to_string(),
            enabled: false,
            reason: Some(base_reason.to_string()),
        },
        ControlInfo {
            action: "module.restart".to_string(),
            label: "Restart".to_string(),
            enabled: false,
            reason: Some(base_reason.to_string()),
        },
    ];

    if let Some(extra_control) = extra {
        controls.push(extra_control);
    }

    controls
}

fn build_config_requirements(config: &Config) -> Vec<ConfigRequirement> {
    let mut requirements = Vec::new();

    if !config.has_graph_config() {
        requirements.push(ConfigRequirement {
            key: "graph".to_string(),
            message: "Graph-Konfiguration fehlt (Inputs/Outputs/Services definieren)".to_string(),
        });
    }

    if config.ringbuffer_required() && config.ringbuffers.is_empty() {
        requirements.push(ConfigRequirement {
            key: "ringbuffer_id".to_string(),
            message: "Ringbuffer-Konfiguration erforderlich".to_string(),
        });
    }

    let codec_ids: HashSet<String> = config
        .codec_instances()
        .into_iter()
        .map(|codec| codec.id)
        .collect();

    for (id, output) in config.outputs.iter().filter(|(_, output)| output.enabled) {
        let Some(codec_id) = output.codec_id.as_deref().filter(|id| !id.is_empty()) else {
            requirements.push(ConfigRequirement {
                key: format!("outputs.{id}.codec_id"),
                message: format!("Output '{}' benötigt codec_id", id),
            });
            continue;
        };

        if !codec_ids.contains(codec_id) {
            requirements.push(ConfigRequirement {
                key: format!("outputs.{id}.codec_id"),
                message: format!("Output '{}' verwendet unbekanntes codec_id '{}'", id, codec_id),
            });
        }
    }

    for (id, service) in config.services.iter().filter(|(_, service)| service.enabled) {
        let requires_codec = matches!(service.service_type.as_str(), "audio_http" | "file_out");
        if !requires_codec {
            continue;
        }

        let Some(codec_id) = service.codec_id.as_deref().filter(|id| !id.is_empty()) else {
            requirements.push(ConfigRequirement {
                key: format!("services.{id}.codec_id"),
                message: format!("Service '{}' benötigt codec_id", id),
            });
            continue;
        };

        if !codec_ids.contains(codec_id) {
            requirements.push(ConfigRequirement {
                key: format!("services.{id}.codec_id"),
                message: format!(
                    "Service '{}' verwendet unbekanntes codec_id '{}'",
                    id, codec_id
                ),
            });
        }
    }

    requirements
}

fn build_file_out_status(
    file_out_spec: Option<FileOutSignalSpec>,
    controls: Vec<ControlInfo>,
    codec_registry: &CodecRegistry,
) -> FileOutStatus {
    let (enabled, path, retention_days, format, current_files) = file_out_spec
        .map(|spec| {
            let current_hour = now_ms() / 1000 / 3600;
            let info = codec_registry.get_info(&spec.codec_id).ok();
            let (format, extension) = info
                .as_ref()
                .map(|info| match info.container {
                    ContainerKind::Ogg => ("ogg".to_string(), "ogg".to_string()),
                    ContainerKind::Mpeg => ("mp3".to_string(), "mp3".to_string()),
                    ContainerKind::Raw => ("raw".to_string(), "raw".to_string()),
                    ContainerKind::Rtp => ("rtp".to_string(), "rtp".to_string()),
                })
                .unwrap_or_else(|| ("–".to_string(), "dat".to_string()));
            let file_path = format!(
                "{}/{}.{}",
                spec.wav_dir.trim_end_matches('/'),
                current_hour,
                extension
            );
            (
                spec.enabled,
                spec.wav_dir,
                spec.retention_days,
                format,
                vec![file_path],
            )
        })
        .unwrap_or((false, "–".to_string(), 0, "–".to_string(), Vec::new()));

    FileOutStatus {
        enabled,
        path,
        format,
        retention_days,
        current_files,
        controls,
    }
}

#[derive(Clone)]
struct FileOutSignalSpec {
    enabled: bool,
    wav_dir: String,
    retention_days: u64,
    codec_id: String,
}

fn find_file_out_signal(config: &Config) -> Option<FileOutSignalSpec> {
    config
        .outputs
        .values()
        .find(|output| output.output_type == "file_out")
        .map(|output| FileOutSignalSpec {
            enabled: output.enabled,
            wav_dir: output.wav_dir.clone().unwrap_or_else(|| "–".to_string()),
            retention_days: output.retention_days.unwrap_or(0),
            codec_id: output.codec_id.clone().unwrap_or_default(),
        })
        .or_else(|| {
            config
                .services
                .values()
                .find(|service| service.service_type == "file_out")
                .map(|service| FileOutSignalSpec {
                    enabled: service.enabled,
                    wav_dir: service.wav_dir.clone().unwrap_or_else(|| "–".to_string()),
                    retention_days: service.retention_days.unwrap_or(0),
                    codec_id: service.codec_id.clone().unwrap_or_default(),
                })
        })
}

fn graph_ringbuffer_id(config: &Config) -> Option<&str> {
    config.ringbuffers.keys().next().map(|id| id.as_str())
}

fn static_snapshot(enabled: bool, running: bool) -> ModuleSnapshot {
    ModuleSnapshot {
        enabled,
        running,
        connected: running,
        counters: CountersSnapshot {
            rx: 0,
            tx: 0,
            drops: 0,
            errors: 0,
        },
        last_activity_ms: 0,
    }
}
