// src/main.rs - KORREKTE VERSION

mod agent;
mod audio;
mod codecs;
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

use log::{debug, error, info, warn};

use config::Config;

use crate::io::peak_analyzer::{PeakAnalyzer, PeakEvent};
use crate::web::influx_service::InfluxHistoryService;
use crate::web::peaks::{PeakPoint, PeakStorage};
use crate::io::influx_out::InfluxOut;
use crate::audio::http::start_audio_http_server;

use crate::recorder::{AudioSink, FsRetention, Mp3Sink, RecorderConfig, WavSink, run_recorder};

use crate::web::run_web_server;

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
    // SRT Input
    // ------------------------------------------------------------
    if let Some(srt_cfg) = &cfg.srt_in {
        if srt_cfg.enabled {
            info!("[airlift] SRT enabled: {}", srt_cfg.listen);
            
            let ring = agent.ring.clone();
            let cfg_clone = srt_cfg.clone();
            let running_srt = running.clone();
            
            std::thread::spawn(move || {
                if let Err(e) = crate::io::srt_in::run_srt_in(ring, cfg_clone, running_srt) {
                    error!("[srt] fatal: {}", e);
                }
            });
        }
    }

    // ------------------------------------------------------------
    // ALSA Input (falls konfiguriert)
    // ------------------------------------------------------------
    if let Some(alsa_cfg) = &cfg.alsa_in {
        if alsa_cfg.enabled {
            info!("[airlift] ALSA enabled: {}", alsa_cfg.device);
            
            let ring = agent.ring.clone();
            let metrics_alsa = metrics.clone();
            
            std::thread::spawn(move || {
                if let Err(e) = crate::io::alsa_in::run_alsa_in(ring, metrics_alsa) {
                    error!("[alsa] fatal: {}", e);
                }
            });
        }
    }

    // ------------------------------------------------------------
    // Peak storage + Web
    // ------------------------------------------------------------
    let peak_store = Arc::new(PeakStorage::new());

    let history_service = cfg.influx_history.as_ref().and_then(|cfg| {
        if cfg.enabled {
            Some(Arc::new(InfluxHistoryService::new(
                cfg.base_url.clone(),
                cfg.token.clone(),
                cfg.org.clone(),
                cfg.bucket.clone(),
            )))
        } else {
            None
        }
    });

    {
        let peak_store_web = peak_store.clone();
        let history_service_web = history_service.clone();
        let ring_buffer = Arc::new(agent.ring.clone());
        let wav_dir = if let Some(rec) = &cfg.recorder {
            PathBuf::from(&rec.wav_dir)
        } else {
            PathBuf::from("/data/aircheck/wav")
        };

        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("web runtime");
            if let Err(e) = rt.block_on(run_web_server(
                peak_store_web, 
                history_service_web,
                ring_buffer,
                wav_dir,
                3008
            )) {
                error!("[web] server error: {}", e);
            }
        });
    }

    info!("[airlift] web enabled (http://localhost:3008)");

// ------------------------------------------------------------
// Audio HTTP Streaming (LIVE + Timeshift) – tiny_http
// ------------------------------------------------------------
{
    let wav_dir = if let Some(rec) = &cfg.recorder {
        PathBuf::from(&rec.wav_dir)
    } else {
        PathBuf::from("/data/aircheck/wav")
    };

    let ring = agent.ring.clone();

    // eigener Thread, blockierend, wie vorgesehen
    std::thread::spawn(move || {
        if let Err(e) = start_audio_http_server(
            "0.0.0.0:3011",          // 🔊 Audio-Port
            wav_dir,
            move || ring.subscribe() // RingReader-Factory
        ) {
            error!("[audio] HTTP server failed: {}", e);
        }
    });

    info!("[airlift] audio HTTP enabled (http://localhost:3011)");
}


    // ------------------------------------------------------------
    // Peaks → Storage
    // ------------------------------------------------------------
    {
        let reader = agent.ring.subscribe();
        let store = peak_store.clone();

let influx = InfluxOut::new(
    "http://localhost:8086/write".into(),
    "rfm_aircheck".into(),
    100, // ms
);

        let handler = Box::new(move |evt: &PeakEvent| {
            influx.handle(evt);
            let peak = PeakPoint {
                timestamp: evt.utc_ns / 1_000_000,
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

fn start_monitoring(
    cfg: &Config,
    agent: &agent::Agent,
    metrics: Arc<monitoring::Metrics>,
    running: Arc<AtomicBool>,
) {
    let ring = agent.ring.clone();
    let port = cfg.monitoring.http_port;

    std::thread::spawn(move || {
        if let Err(e) = monitoring::run_metrics_server(metrics, ring, port, running) {
            error!("[monitoring] error: {}", e);
        }
    });

    info!("[airlift] monitoring on port {}", port);
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

    let retentions: Vec<Box<dyn recorder::RetentionPolicy>> = vec![Box::new(FsRetention::new(
        wav_dir.clone(),
        c.retention_days,
    ))];

    let rec_cfg = RecorderConfig {
        idle_sleep: Duration::from_millis(5),
        retention_interval: Duration::from_secs(3600),
        continuity_interval: Duration::from_millis(100),
    };

    std::thread::spawn(move || {
        if let Err(e) = run_recorder(reader, rec_cfg, sinks, retentions) {
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
