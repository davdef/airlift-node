// src/main.rs

mod agent;
mod audio;
mod config;
mod io;
mod monitoring;
mod recorder;
mod ring;
mod web;

use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use log::{debug, error, info};

use config::Config;

use crate::io::peak_analyzer::{PeakAnalyzer, PeakEvent};
use crate::web::peaks::{PeakPoint, PeakStorage};

use crate::recorder::{
    run_recorder, AudioSink, Mp3Sink, WavSink,
    RecorderConfig, FsRetention,
};

use crate::audio::http::start_audio_http_server;
use crate::web::run_web_server;

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info"),
    )
    .format_timestamp_millis()
    .init();

    // ------------------------------------------------------------
    // Config
    // ------------------------------------------------------------
    let cfg_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config.toml".into());

    let cfg: Config = config::load(&cfg_path)?;
    info!("[airlift] loaded {}", cfg_path);

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
    // Agent / Ring
    // ------------------------------------------------------------
    let agent = agent::Agent::new(cfg.ring.slots, cfg.ring.prealloc_samples);

    // ------------------------------------------------------------
    // Monitoring
    // ------------------------------------------------------------
    monitoring::create_health_file()?;
    let metrics = Arc::new(monitoring::Metrics::new());
    start_monitoring(&cfg, &agent, metrics.clone(), running.clone());

    // ------------------------------------------------------------
    // Peak storage + Web
    // ------------------------------------------------------------
    let peak_store = Arc::new(PeakStorage::new());

    {
        let peak_store_web = peak_store.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new()
                .expect("web runtime");
            if let Err(e) = rt.block_on(run_web_server(peak_store_web, 3008)) {
                error!("[web] server error: {}", e);
            }
        });
    }

    info!("[airlift] web enabled (http://localhost:3008)");

    // ------------------------------------------------------------
    // Peaks → Storage
    // ------------------------------------------------------------
    {
        let reader = agent.ring.subscribe();
        let store = peak_store.clone();

        let handler = Box::new(move |evt: &PeakEvent| {
            let peak = PeakPoint {
                timestamp: evt.utc_ns,
                peak_l: evt.peak_l,
                peak_r: evt.peak_r,
                rms: None,
                lufs: None,
                silence: evt.silence,
            };
            store.add_peak(peak);
        });

        let mut analyzer = PeakAnalyzer::new(reader, handler, 0);
        std::thread::spawn(move || analyzer.run());

        info!("[airlift] peak_storage enabled");
    }

    // ------------------------------------------------------------
    // Recorder (optional, aber stabil)
    // ------------------------------------------------------------
    start_recorder(&cfg, &agent)?;

    // ------------------------------------------------------------
    // Audio HTTP
    // ------------------------------------------------------------
    start_audio_http(&agent)?;

    // ------------------------------------------------------------
    // Main loop
    // ------------------------------------------------------------
    info!("[airlift] running – Ctrl+C to stop");

    let mut last_stats = Instant::now();

    while running.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(100));

        if last_stats.elapsed() >= Duration::from_secs(5) {
            let stats = agent.ring.stats();
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
    monitoring::update_health_status(false)?;
    info!("[airlift] shutdown complete");

    Ok(())
}

//
// ============================================================
// Helpers
// ============================================================
//

fn start_monitoring(
    cfg: &Config,
    agent: &agent::Agent,
    metrics: Arc<monitoring::Metrics>,
    running: Arc<AtomicBool>,
) {
    let ring = agent.ring.clone();
    let port = cfg.monitoring.http_port;

    std::thread::spawn(move || {
        if let Err(e) =
            monitoring::run_metrics_server(metrics, ring, port, running)
        {
            error!("[monitoring] error: {}", e);
        }
    });

    info!("[airlift] monitoring on port {}", port);
}

fn start_audio_http(agent: &agent::Agent) -> anyhow::Result<()> {
    let ring = agent.ring.clone();

    start_audio_http_server(
        "0.0.0.0:3007",
        PathBuf::from("/data/aircheck/wav"),
        move || ring.subscribe(),
    )?;

    info!("[airlift] audio_http enabled (0.0.0.0:3007)");
    Ok(())
}

fn start_recorder(cfg: &Config, agent: &agent::Agent) -> anyhow::Result<()> {
    let Some(c) = &cfg.recorder else {
        return Ok(());
    };
    if !c.enabled {
        return Ok(());
    }

    let reader = agent.ring.subscribe();

    let mut sinks: Vec<Box<dyn AudioSink>> = Vec::new();

    let wav_dir = PathBuf::from(&c.wav_dir);
    sinks.push(Box::new(WavSink::new(wav_dir.clone())?));

    if let Some(mp3) = &c.mp3 {
        sinks.push(Box::new(Mp3Sink::new(
            wav_dir.clone(),
            PathBuf::from(&mp3.dir),
            mp3.bitrate,
        )?));
    }

    let retentions: Vec<Box<dyn recorder::RetentionPolicy>> =
        vec![Box::new(FsRetention::new(
            wav_dir.clone(),
            c.retention_days,
        ))];

    let rec_cfg = RecorderConfig {
        idle_sleep: Duration::from_millis(5),
        retention_interval: Duration::from_secs(3600),
        continuity_interval: Duration::from_millis(100),
    };

    std::thread::spawn(move || {
        if let Err(e) =
            run_recorder(reader, rec_cfg, sinks, retentions)
        {
            error!("[recorder] fatal: {}", e);
        }
    });

    info!(
        "[airlift] recorder enabled (wav{}, retention {}d)",
        if c.mp3.is_some() { "+mp3" } else { "" },
        c.retention_days
    );

    Ok(())
}
