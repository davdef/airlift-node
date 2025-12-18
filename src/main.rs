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
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use config::Config;
use log::{debug, error, info};

use crate::io::peak_analyzer::{PeakAnalyzer, PeakEvent};
use crate::web::influx_service::InfluxService;
use crate::web::peak_storage::PeakStorage;

use crate::recorder::{AudioSink, Mp3Sink, WavSink, run_recorder};
use crate::recorder::RecorderConfig;
use crate::recorder::FsRetention;

use crate::audio::http::start_audio_http_server;

use crate::web::run_web_server;
use crate::web::peaks::{PeakStorage, PeakPoint};
use crate::web::influx_service::InfluxService;

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
    // Peak Storage und Web Server
    // ------------------------------------------------------------
    let peak_store = Arc::new(PeakStorage::new());
    
    // Web Server starten (auf anderem Port, z.B. 3008)
    let peak_store_web = peak_store.clone();
    std::thread::spawn(move || {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            if let Err(e) = run_web_server(peak_store_web, 3008).await {
                error!("[web] server error: {}", e);
            }
        });
    });
    
    info!("[airlift] web player enabled (http://localhost:3008/player)");

let influx_service = Arc::new(InfluxService::new(
    "http://localhost:8086".to_string(),
    "".to_string(), // Token - anpassen falls benötigt
    "rfm_aircheck".to_string(),
    "rfm_aircheck".to_string(),
));

let peak_store_web = peak_store.clone();
let influx_service_web = influx_service.clone();
std::thread::spawn(move || {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        if let Err(e) = run_web_server(peak_store_web, 3008).await {
            error!("[web] server error: {}", e);
        }
    });
});

    // ------------------------------------------------------------
    // IO NODES
    // ------------------------------------------------------------
    start_alsa_in(&cfg, &agent, metrics.clone());
    start_udp_out(&cfg, &agent, metrics.clone());
    start_icecast_out(&cfg, &agent);
    start_mp3_out(&cfg, &agent);
    start_srt_in(&cfg, &agent, running.clone());
    start_srt_out(&cfg, &agent, running.clone());

    // ------------------------------------------------------------
    // Peaks / Influx / Broadcast
    // ------------------------------------------------------------
    start_peak_console(&agent);
<<<<<<< HEAD
    start_web_layer(&agent);
=======
    start_peak_broadcast(&agent);
    
    // Combined handler mit Peak Storage
    {
        let reader = agent.ring.subscribe();
        let peak_store_handler = peak_store.clone();
        
        // Handler mit lokaler Erstellung der Handler
        let handler = Box::new(move |evt: &PeakEvent| {
            // Peak in Storage speichern (konvertiere PeakEvent zu PeakPoint)
            let peak = PeakPoint {
                timestamp: evt.utc_ns,
                peak_l: evt.peak_l,
                peak_r: evt.peak_r,
                rms: None,
                lufs: None,
                silence: evt.silence,
            };
            peak_store_handler.add_peak(peak);
            
            // Lokale Handler erstellen (da Clone nicht verfügbar)
            let influx = InfluxOut::new(
                "http://localhost:8086/write".to_string(),
                "rfm_aircheck".to_string(),
                100,
            );
            
            let broadcaster = BroadcastHttp::new(
                "http://localhost:3006/api/broadcast".to_string(),
                100,
            );
            
            influx.handle(evt);
            broadcaster.handle(evt);
        });

        let mut analyzer = PeakAnalyzer::new(reader, handler, 0);
        std::thread::spawn(move || analyzer.run());

        info!("[airlift] influx_out + broadcast_http + peak_storage enabled");
    }

>>>>>>> ffd6f69 (Frontend integration)
    // ------------------------------------------------------------
    // Recorder (WAV / MP3 / Retention)
    // ------------------------------------------------------------
    start_recorder(&cfg, &agent)?;

    let ring = agent.ring.clone();

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
// START_* HELPERS
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
        if let Err(e) = monitoring::run_metrics_server(metrics, ring, port, running) {
            error!("[monitoring] error: {}", e);
        }
    });

    info!("[airlift] monitoring on port {}", port);
}

fn start_alsa_in(cfg: &Config, agent: &agent::Agent, metrics: Arc<monitoring::Metrics>) {
    if let Some(c) = &cfg.alsa_in {
        if !c.enabled {
            return;
        }

        let ring = agent.ring.clone();
        std::thread::spawn(move || {
            if let Err(e) = io::alsa_in::run_alsa_in(ring, metrics) {
                error!("[alsa_in] fatal: {}", e);
            }
        });

        info!("[airlift] alsa_in enabled ({})", c.device);
    }
}

fn start_udp_out(cfg: &Config, agent: &agent::Agent, metrics: Arc<monitoring::Metrics>) {
    if let Some(c) = &cfg.udp_out {
        if !c.enabled {
            return;
        }

        let reader = agent.ring.subscribe();
        let target = c.target.clone();

        std::thread::spawn(move || {
            if let Err(e) = io::udp_out::run_udp_out(reader, &target, metrics) {
                error!("[udp_out] fatal: {}", e);
            }
        });

        info!("[airlift] udp_out → {}", c.target);
    }
}

fn start_icecast_out(cfg: &Config, agent: &agent::Agent) {
    if let Some(c) = &cfg.icecast_out {
        if !c.enabled {
            return;
        }

        let reader = agent.ring.subscribe();

        let ice_cfg = io::icecast_out::IcecastConfig {
            host: c.host.clone(),
            port: c.port,
            mount: c.mount.clone(),
            user: c.user.clone(),
            password: c.password.clone(),
            name: c.name.clone(),
            description: c.description.clone(),
            genre: c.genre.clone(),
            public: c.public,
            opus_bitrate: c.bitrate,
        };

        std::thread::spawn(move || {
            if let Err(e) = io::icecast_out::run_icecast_out(reader, ice_cfg) {
                error!("[icecast_out] fatal: {}", e);
            }
        });

        info!("[airlift] icecast_out → {}:{}{}", c.host, c.port, c.mount);
    }
}

fn start_mp3_out(cfg: &Config, agent: &agent::Agent) {
    if let Some(c) = &cfg.mp3_out {
        if !c.enabled {
            return;
        }

        let reader = agent.ring.subscribe();

        let mp3_cfg = io::mp3_out::Mp3Config {
            host: c.host.clone(),
            port: c.port,
            mount: c.mount.clone(),
            user: c.user.clone(),
            password: c.password.clone(),
            name: c.name.clone(),
            description: c.description.clone(),
            genre: c.genre.clone(),
            public: c.public,
            bitrate: c.bitrate,
        };

        std::thread::spawn(move || {
            if let Err(e) = io::mp3_out::run_mp3_out(reader, mp3_cfg) {
                error!("[mp3_out] fatal: {}", e);
            }
        });

        info!(
            "[airlift] mp3_out → {}:{}{} ({} kbps)",
            c.host, c.port, c.mount, c.bitrate
        );
    }
}

fn start_srt_in(cfg: &Config, agent: &agent::Agent, running: Arc<AtomicBool>) {
    if let Some(c) = &cfg.srt_in {
        if !c.enabled {
            return;
        }

        let ring = agent.ring.clone();
        let cfg = c.clone();
        let running = running.clone();

        std::thread::spawn(move || {
            if let Err(e) = io::srt_in::run_srt_in(ring, cfg, running) {
                error!("[srt_in] fatal: {}", e);
            }
        });

        info!("[airlift] srt_in → {}", c.listen);
    }
}

fn start_srt_out(cfg: &Config, agent: &agent::Agent, running: Arc<AtomicBool>) {
    if let Some(c) = cfg.srt_out.clone() {
        let reader = agent.ring.subscribe();
        let target = c.target.clone();
        let running = running.clone();

        std::thread::spawn(move || {
            if let Err(e) = io::srt_out::run_srt_out(reader, c, running) {
                error!("[srt_out] fatal: {}", e);
            }
        });

        info!("[airlift] srt_out → {}", target);
    }
}

fn start_peak_console(agent: &agent::Agent) {
    let reader = agent.ring.subscribe();

    let handler = Box::new(|evt: &PeakEvent| {
        debug!(
            "[peak] seq={} L={:.3} R={:.3} lat={:.1}ms",
            evt.seq, evt.peak_l, evt.peak_r, evt.latency_ms
        );
    });

    let mut analyzer = PeakAnalyzer::new(reader, handler, 100);
    std::thread::spawn(move || analyzer.run());

    info!("[airlift] peak_console enabled");
}

<<<<<<< HEAD
fn start_web_layer(agent: &agent::Agent) {
    let peaks = Arc::new(PeakStorage::new(1000));
    let influx = Arc::new(InfluxService::new(
        "http://localhost:8086/write".to_string(),
        "rfm_aircheck".to_string(),
        100,
    ));

    let state = web::AppState {
        peaks: peaks.clone(),
        influx: influx.clone(),
    };

    let reader = agent.ring.subscribe();
    let app_state = state.clone();
    std::thread::spawn(move || {
        let handler = Box::new(move |evt: &PeakEvent| {
            web::merge_peak_into_state(&app_state, evt.clone());
        });

        let mut analyzer = PeakAnalyzer::new(reader, handler, 0);
        analyzer.run();
    });

    std::thread::spawn(move || {
        let addr = "0.0.0.0:3006".parse().unwrap();
        let rt = tokio::runtime::Runtime::new().expect("web runtime");
        if let Err(err) = rt.block_on(web::serve(state, addr)) {
            error!("[web] server error: {}", err);
        }
    });

    info!("[airlift] web layer enabled (0.0.0.0:3006)");
=======
fn start_peak_broadcast(agent: &agent::Agent) {
    let reader = agent.ring.subscribe();

    let broadcaster = BroadcastHttp::new("http://localhost:3006/api/broadcast".to_string(), 100);

    let handler = Box::new(move |evt: &PeakEvent| {
        broadcaster.handle(evt);
    });

    let mut analyzer = PeakAnalyzer::new(reader, handler, 0);
    std::thread::spawn(move || analyzer.run());

    info!("[airlift] broadcast_http enabled");
>>>>>>> ffd6f69 (Frontend integration)
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
