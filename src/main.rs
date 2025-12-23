// src/main.rs - Bootstrap-Orchestrator

mod agent;
mod api;
mod audio;
mod bootstrap;
mod codecs;
mod config;
mod container;
mod control;
mod decoder;
mod io;
mod monitoring;
mod ring;
mod services;
mod web;

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use log::{debug, info};

use crate::api::{ApiService, ApiState, Registry};
use crate::bootstrap::{AppContext, register_graph_modules, start_graph_workers};
use crate::config::Config;
use crate::services::{MonitoringService, register_graph_services};

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    // ------------------------------------------------------------
    // Config
    // ------------------------------------------------------------
    let cfg_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config.toml".into());

    let cfg: Arc<Config> = Arc::new(config::load(&cfg_path)?);
    info!("[airlift] loaded {}", cfg_path);

    let graph = cfg.validate_graph()?;
    let graph = match graph {
        Some(graph) => graph,
        None => {
            if cfg.api.enabled || cfg.monitoring.enabled || cfg.audio_http.enabled {
                anyhow::bail!("graph configuration required for enabled services");
            }
            info!("[airlift] no graph configuration; idle mode");
            let running = Arc::new(AtomicBool::new(true));
            {
                let r = running.clone();
                ctrlc::set_handler(move || {
                    info!("\n[airlift] shutdown requested");
                    r.store(false, Ordering::SeqCst);
                })?;
            }
            while running.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(100));
            }
            info!("[airlift] shutdown complete");
            return Ok(());
        }
    };
    let needs_agent = cfg.api.enabled
        || cfg.monitoring.enabled
        || cfg.audio_http.enabled
        || graph.inputs.values().any(|input| input.enabled)
        || graph.outputs.values().any(|output| output.enabled)
        || graph.services.values().any(|service| service.enabled);

    // ------------------------------------------------------------
    // Graceful shutdown
    // ------------------------------------------------------------
    let running = Arc::new(AtomicBool::new(true));
    {
        let r = running.clone();
        ctrlc::set_handler(move || {
            info!("\n[airlift] shutdown requested");
            r.store(false, Ordering::SeqCst);
        })?;
    }

    if !needs_agent {
        info!("[airlift] no components enabled; idle mode");
        while running.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(100));
        }
        info!("[airlift] shutdown complete");
        return Ok(());
    }

    // ------------------------------------------------------------
    // Bootstrap context (shared state)
    // ------------------------------------------------------------
    let ring_config = crate::config::RingConfig {
        slots: graph.ringbuffer.slots,
        prealloc_samples: graph.ringbuffer.prealloc_samples,
    };
    let agent = agent::Agent::new(ring_config.slots, ring_config.prealloc_samples);
    let codec_registry =
        Arc::new(crate::codecs::registry::CodecRegistry::new(graph.codecs.clone()));
    let context = AppContext::new(&cfg, agent, codec_registry.clone());

    // ------------------------------------------------------------
    // API registry (modules + services)
    // ------------------------------------------------------------
    let registry = Arc::new(Registry::new());
    register_graph_modules(&graph, &registry);

    // ------------------------------------------------------------
    // Services
    // ------------------------------------------------------------
    let api_bind = cfg.api.bind.as_str();
    let audio_bind = cfg.audio_http.bind.as_str();

    register_graph_services(
        &registry,
        api_bind,
        audio_bind,
        cfg.monitoring.http_port,
        cfg.api.enabled,
        cfg.audio_http.enabled,
        cfg.monitoring.enabled,
        &graph,
    );

    if cfg.api.enabled {
        let api_state = ApiState {
            peak_store: context.peak_store.clone(),
            history_service: context.history_service.clone(),
            ring: context.agent.ring.clone(),
            encoded_ring: context.agent.encoded_ring.clone(),
            control_state: context.control_state.clone(),
            config: cfg.clone(),
            registry: registry.clone(),
            wav_dir: context.wav_dir.clone(),
            codec_registry: context.codec_registry.clone(),
        };

        let api_service = ApiService::new(api_bind.parse()?);
        api_service.start(api_state);
    }

    let mut monitoring_started = false;
    let graph_monitoring_enabled = graph
        .services
        .iter()
        .find(|(_, svc)| svc.service_type == "monitoring")
        .is_some_and(|(_, svc)| svc.enabled);

    start_graph_workers(&cfg, &graph, &context, running.clone())?;
    monitoring_started = cfg.monitoring.enabled && graph_monitoring_enabled;

    // ------------------------------------------------------------
    // Main loop
    // ------------------------------------------------------------
    info!("[airlift] running – Ctrl+C to stop");

    let mut last_stats = Instant::now();

    while running.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(100));

        if last_stats.elapsed() >= Duration::from_secs(5) {
            let stats = context.agent.ring.stats();
            let fill = stats.head_seq - stats.next_seq.wrapping_sub(1);
            debug!("[airlift] head_seq={} fill={}", stats.head_seq, fill);
            last_stats = Instant::now();
        }
    }

    // ------------------------------------------------------------
    // Shutdown
    // ------------------------------------------------------------
    info!("[airlift] shutting down…");
    std::thread::sleep(Duration::from_secs(1));
    if monitoring_started {
        MonitoringService::mark_shutdown()?;
    }
    info!("[airlift] shutdown complete");

    Ok(())
}
