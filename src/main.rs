// src/main.rs - Bootstrap-Orchestrator

mod agent;
mod api;
mod audio;
mod bootstrap;
mod codecs;
mod config;
mod control;
mod io;
mod monitoring;
mod recorder;
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
use crate::bootstrap::{
    AppContext, register_graph_modules, register_modules, start_graph_workers, start_workers,
};
use crate::config::Config;
use crate::services::{
    AudioHttpService, MonitoringService, register_graph_services, register_services,
};

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

    // ------------------------------------------------------------
    // Bootstrap context (shared state)
    // ------------------------------------------------------------
    let ring_config = graph
        .as_ref()
        .map(|graph| crate::config::RingConfig {
            slots: graph.ringbuffer.slots,
            prealloc_samples: graph.ringbuffer.prealloc_samples,
        })
        .unwrap_or_else(|| cfg.ring.clone());
    let agent = agent::Agent::new(ring_config.slots, ring_config.prealloc_samples);
    let codec_registry = Arc::new(if let Some(graph) = &graph {
        crate::codecs::registry::CodecRegistry::new(graph.codecs.clone())
    } else {
        crate::codecs::registry::CodecRegistry::from_config(&cfg)
    });
    let context = AppContext::new(&cfg, agent, codec_registry.clone());

    // ------------------------------------------------------------
    // API registry (modules + services)
    // ------------------------------------------------------------
    let registry = Arc::new(Registry::new());
    if let Some(graph) = &graph {
        register_graph_modules(graph, &registry);
    } else {
        register_modules(&cfg, &registry);
    }

    // ------------------------------------------------------------
    // Services
    // ------------------------------------------------------------
    let api_bind = "0.0.0.0:3008";
    let audio_bind = "0.0.0.0:3011";

    if let Some(graph) = &graph {
        register_graph_services(&registry, api_bind, audio_bind, cfg.monitoring.http_port, graph);
    } else {
        register_services(&registry, api_bind, audio_bind, cfg.monitoring.http_port);
    }

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

    if let Some(graph) = &graph {
        start_graph_workers(&cfg, graph, &context, running.clone())?;
    } else {
        MonitoringService::start(
            &cfg,
            &context.agent,
            context.metrics.clone(),
            running.clone(),
        )?;

        let audio_http_service = AudioHttpService::new(audio_bind);
        audio_http_service.start(
            context.wav_dir.clone(),
            context.agent.ring.clone(),
            cfg.audio_http_codec_id.clone(),
            context.codec_registry.clone(),
        );

        // ------------------------------------------------------------
        // Module workers (audio pipeline, recorder, peaks)
        // ------------------------------------------------------------
        start_workers(&cfg, &context, running.clone())?;
    }

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
    MonitoringService::mark_shutdown()?;
    info!("[airlift] shutdown complete");

    Ok(())
}
