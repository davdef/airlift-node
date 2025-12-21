use axum::{
    Json,
    extract::State,
    response::sse::{Event, Sse},
};
use serde::Serialize;
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use tokio::time::{Duration, interval};
use tokio_stream::{StreamExt, wrappers::IntervalStream};

use crate::api::ApiState;
use crate::codecs::registry::{
    CodecInstanceSnapshot, CodecRegistry, DEFAULT_CODEC_OPUS_OGG_ID, DEFAULT_CODEC_PCM_ID,
};
use crate::config::Config;
use crate::control::{ControlState, ModuleSnapshot, now_ms};
use crate::ring::RingStats;

#[derive(Serialize)]
pub struct RingStatus {
    pub capacity: usize,
    pub head_seq: u64,
    pub next_seq: u64,
    pub fill: u64,
    pub head_index: u64,
    pub tail_index: u64,
    pub fill_ratio: f64,
}

#[derive(Serialize)]
pub struct ControlInfo {
    pub action: String,
    pub label: String,
    pub enabled: bool,
    pub reason: Option<String>,
}

#[derive(Serialize)]
pub struct ModuleInfo {
    pub id: String,
    pub label: String,
    #[serde(rename = "type")]
    pub module_type: String,
    pub runtime: ModuleSnapshot,
    pub controls: Vec<ControlInfo>,
}

#[derive(Serialize)]
pub struct InactiveModule {
    pub id: String,
    pub label: String,
    #[serde(rename = "type")]
    pub module_type: String,
    pub reason: String,
    pub can_activate: bool,
    pub activate_action: Option<String>,
}

#[derive(Serialize)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub kind: String,
}

#[derive(Serialize)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
}

#[derive(Serialize)]
pub struct GraphStatus {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

#[derive(Serialize)]
pub struct RecorderStatus {
    pub enabled: bool,
    pub path: String,
    pub format: String,
    pub retention_days: u64,
    pub current_files: Vec<String>,
    pub controls: Vec<ControlInfo>,
}

#[derive(Serialize)]
pub struct StatusResponse {
    pub timestamp_ms: u64,
    pub ring: RingStatus,
    pub modules: Vec<ModuleInfo>,
    pub inactive_modules: Vec<InactiveModule>,
    pub graph: GraphStatus,
    pub recorder: RecorderStatus,
}

pub async fn get_status(State(state): State<ApiState>) -> Json<StatusResponse> {
    Json(build_status(
        &state.control_state,
        &state.ring.stats(),
        &state.config,
        &state.codec_registry,
    ))
}

pub async fn events(
    State(state): State<ApiState>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let control_state = state.control_state.clone();
    let ring = state.ring.clone();
    let config = state.config.clone();
    let codec_registry = state.codec_registry.clone();

    let stream = IntervalStream::new(interval(Duration::from_secs(1))).map(move |_| {
        let status = build_status(&control_state, &ring.stats(), &config, &codec_registry);
        let data = serde_json::to_string(&status).unwrap_or_else(|_| "{}".to_string());
        Ok(Event::default().event("status").data(data))
    });

    Sse::new(stream)
}

fn build_status(
    control_state: &Arc<ControlState>,
    ring_stats: &RingStats,
    config: &Config,
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

    let srt_in = control_state.srt_in.module.snapshot();
    let srt_out = control_state.srt_out.module.snapshot();
    let alsa_in = control_state.alsa_in.snapshot();
    let icecast_out = control_state.icecast_out.snapshot();
    let recorder = control_state.recorder.snapshot();
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

    let recorder_controls = build_controls("recorder", &recorder, "Vorbereitet", None);

    add_module_if_active(
        &mut modules,
        "ring",
        "Ringbuffer",
        "buffer",
        ring_module.clone(),
        build_controls("ring", &ring_module, "Nicht unterstützt", None),
    );
    add_module_if_active(
        &mut modules,
        "srt_in",
        "SRT-IN",
        "input",
        srt_in.clone(),
        srt_in_controls.clone(),
    );
    add_module_if_active(
        &mut modules,
        "alsa_in",
        "ALSA-IN",
        "input",
        alsa_in.clone(),
        build_controls("alsa_in", &alsa_in, "Nicht unterstützt", None),
    );
    add_module_if_active(
        &mut modules,
        "srt_out",
        "SRT-OUT",
        "output",
        srt_out.clone(),
        srt_out_controls.clone(),
    );
    add_module_if_active(
        &mut modules,
        "icecast_out",
        "Icecast-Out",
        "output",
        icecast_out.clone(),
        build_controls("icecast_out", &icecast_out, "Nicht unterstützt", None),
    );
    add_module_if_active(
        &mut modules,
        "recorder",
        "Recorder",
        "output",
        recorder.clone(),
        recorder_controls.clone(),
    );

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
            });
        }
    }

    add_inactive_module(
        &mut inactive_modules,
        "srt_in",
        "SRT-IN",
        "input",
        &srt_in,
        config.srt_in.as_ref().map(|cfg| cfg.enabled),
    );
    add_inactive_module(
        &mut inactive_modules,
        "alsa_in",
        "ALSA-IN",
        "input",
        &alsa_in,
        config.alsa_in.as_ref().map(|cfg| cfg.enabled),
    );
    add_inactive_module(
        &mut inactive_modules,
        "srt_out",
        "SRT-OUT",
        "output",
        &srt_out,
        config.srt_out.as_ref().map(|cfg| cfg.enabled),
    );
    add_inactive_module(
        &mut inactive_modules,
        "icecast_out",
        "Icecast-Out",
        "output",
        &icecast_out,
        config.icecast_out.as_ref().map(|cfg| cfg.enabled),
    );
    add_inactive_module(
        &mut inactive_modules,
        "recorder",
        "Recorder",
        "output",
        &recorder,
        config.recorder.as_ref().map(|cfg| cfg.enabled),
    );

    let graph = build_graph(
        &srt_in,
        &alsa_in,
        &srt_out,
        &icecast_out,
        &recorder,
        &ring_module,
        config,
        &codec_map,
    );

    let recorder_status = build_recorder_status(config, recorder_controls);

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
        },
        modules,
        inactive_modules,
        graph,
        recorder: recorder_status,
    }
}

fn add_module_if_active(
    modules: &mut Vec<ModuleInfo>,
    id: &str,
    label: &str,
    module_type: &str,
    snapshot: ModuleSnapshot,
    controls: Vec<ControlInfo>,
) {
    if snapshot.enabled && snapshot.running {
        modules.push(ModuleInfo {
            id: id.to_string(),
            label: label.to_string(),
            module_type: module_type.to_string(),
            runtime: snapshot,
            controls,
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
    alsa_in: &ModuleSnapshot,
    srt_out: &ModuleSnapshot,
    icecast_out: &ModuleSnapshot,
    recorder: &ModuleSnapshot,
    ring: &ModuleSnapshot,
    config: &Config,
    codec_map: &HashMap<String, CodecInstanceSnapshot>,
) -> GraphStatus {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    let ring_active = ring.enabled && ring.running;
    if ring_active {
        nodes.push(GraphNode {
            id: "ring".to_string(),
            label: "Ringbuffer".to_string(),
            kind: "buffer".to_string(),
        });
    }

    if srt_in.enabled && srt_in.running {
        nodes.push(GraphNode {
            id: "srt_in".to_string(),
            label: "SRT-IN".to_string(),
            kind: "input".to_string(),
        });
        if ring_active {
            edges.push(GraphEdge {
                from: "srt_in".to_string(),
                to: "ring".to_string(),
            });
        }
    }

    if alsa_in.enabled && alsa_in.running {
        nodes.push(GraphNode {
            id: "alsa_in".to_string(),
            label: "ALSA-IN".to_string(),
            kind: "input".to_string(),
        });
        if ring_active {
            edges.push(GraphEdge {
                from: "alsa_in".to_string(),
                to: "ring".to_string(),
            });
        }
    }

    add_output_to_graph(
        &mut nodes,
        &mut edges,
        "srt_out",
        "SRT-OUT",
        srt_out,
        ring_active,
        config
            .srt_out
            .as_ref()
            .and_then(|cfg| cfg.codec_id.clone())
            .unwrap_or_else(|| DEFAULT_CODEC_PCM_ID.to_string()),
        codec_map,
    );

    add_output_to_graph(
        &mut nodes,
        &mut edges,
        "icecast_out",
        "Icecast-Out",
        icecast_out,
        ring_active,
        config
            .icecast_out
            .as_ref()
            .and_then(|cfg| cfg.codec_id.clone())
            .unwrap_or_else(|| DEFAULT_CODEC_OPUS_OGG_ID.to_string()),
        codec_map,
    );

    if recorder.enabled && recorder.running {
        nodes.push(GraphNode {
            id: "recorder".to_string(),
            label: "Recorder".to_string(),
            kind: "output".to_string(),
        });
        if ring_active {
            edges.push(GraphEdge {
                from: "ring".to_string(),
                to: "recorder".to_string(),
            });
        }
    }

    GraphStatus { nodes, edges }
}

fn add_output_to_graph(
    nodes: &mut Vec<GraphNode>,
    edges: &mut Vec<GraphEdge>,
    id: &str,
    label: &str,
    snapshot: &ModuleSnapshot,
    ring_active: bool,
    codec_id: String,
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

    let codec_snapshot = codec_map.get(&codec_id);
    let codec_active = codec_snapshot
        .map(|snapshot| snapshot.runtime_state.enabled && snapshot.runtime_state.running)
        .unwrap_or(false);

    if ring_active {
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
                from: "ring".to_string(),
                to: codec_node_id.clone(),
            });
            edges.push(GraphEdge {
                from: codec_node_id,
                to: id.to_string(),
            });
        } else {
            edges.push(GraphEdge {
                from: "ring".to_string(),
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

fn build_recorder_status(config: &Config, controls: Vec<ControlInfo>) -> RecorderStatus {
    let (enabled, path, retention_days, format, current_files) = match &config.recorder {
        Some(cfg) => {
            let mut files = Vec::new();
            let current_hour = now_ms() / 1000 / 3600;
            let wav_path = format!("{}/{}.wav", cfg.wav_dir.trim_end_matches('/'), current_hour);
            files.push(wav_path);
            let mut format = "wav".to_string();
            if let Some(mp3) = &cfg.mp3 {
                let mp3_path = format!("{}/{}.mp3", mp3.dir.trim_end_matches('/'), current_hour);
                files.push(mp3_path);
                format.push_str(" + mp3");
            }
            (
                cfg.enabled,
                cfg.wav_dir.clone(),
                cfg.retention_days,
                format,
                files,
            )
        }
        None => (false, "–".to_string(), 0, "–".to_string(), Vec::new()),
    };

    RecorderStatus {
        enabled,
        path,
        format,
        retention_days,
        current_files,
        controls,
    }
}
