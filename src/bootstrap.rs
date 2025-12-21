use std::path::PathBuf;
use std::sync::{Arc, atomic::AtomicBool};
use std::time::Duration;

use log::{error, info};

use crate::agent;
use crate::api::registry::{ModuleDescriptor, ModuleRegistration, Registry};
use crate::config::Config;
use crate::control::ControlState;
use crate::io::influx_out::InfluxOut;
use crate::io::peak_analyzer::{PeakAnalyzer, PeakEvent};
use crate::monitoring;
use crate::recorder::{
    self, AudioSink, FsRetention, Mp3Sink, RecorderConfig, WavSink, run_recorder,
};
use crate::web::influx_service::InfluxHistoryService;
use crate::web::peaks::{PeakPoint, PeakStorage};

pub struct AppContext {
    pub agent: agent::Agent,
    pub metrics: Arc<monitoring::Metrics>,
    pub control_state: Arc<ControlState>,
    pub peak_store: Arc<PeakStorage>,
    pub history_service: Option<Arc<InfluxHistoryService>>,
    pub wav_dir: PathBuf,
}

impl AppContext {
    pub fn new(cfg: &Config) -> Self {
        let agent = agent::Agent::new(cfg.ring.slots, cfg.ring.prealloc_samples);
        let metrics = Arc::new(monitoring::Metrics::new());

        let control_state = Arc::new(ControlState::new());
        control_state.ring.set_enabled(true);
        control_state.ring.set_running(true);
        control_state.ring.set_connected(true);

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

        let wav_dir = cfg
            .recorder
            .as_ref()
            .map(|rec| PathBuf::from(&rec.wav_dir))
            .unwrap_or_else(|| PathBuf::from("/data/aircheck/wav"));

        Self {
            agent,
            metrics,
            control_state,
            peak_store,
            history_service,
            wav_dir,
        }
    }
}

pub fn register_modules(cfg: &Config, registry: &Registry) {
    let ring_descriptor = ModuleDescriptor::new("ring", "ring", "running", "running");
    registry.register_module(ModuleRegistration::new(ring_descriptor));

    register_toggle_module(
        registry,
        "srt_in",
        "srt_in",
        cfg.srt_in.as_ref().map(|c| c.enabled),
    );
    register_toggle_module(
        registry,
        "srt_out",
        "srt_out",
        cfg.srt_out.as_ref().map(|c| c.enabled),
    );
    register_toggle_module(
        registry,
        "alsa_in",
        "alsa_in",
        cfg.alsa_in.as_ref().map(|c| c.enabled),
    );
    register_toggle_module(
        registry,
        "icecast_out",
        "icecast_out",
        cfg.icecast_out.as_ref().map(|c| c.enabled),
    );
    register_toggle_module(
        registry,
        "recorder",
        "recorder",
        cfg.recorder.as_ref().map(|c| c.enabled),
    );

    // Codec-Module als eigenständige Processing-Instanzen.
    register_codec(registry, "codec_opus", true);
    let mp3_enabled = cfg
        .recorder
        .as_ref()
        .and_then(|rec| rec.mp3.as_ref())
        .is_some()
        || cfg.mp3_out.as_ref().map(|m| m.enabled).unwrap_or(false);
    register_codec(registry, "codec_mp3", mp3_enabled);
}

pub fn start_workers(
    cfg: &Config,
    context: &AppContext,
    running: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    start_srt_in(cfg, context, running.clone());
    start_alsa_in(cfg, context);
    sync_output_states(cfg, context);
    start_peak_storage(context);
    start_recorder(cfg, context)?;
    Ok(())
}

fn register_toggle_module(registry: &Registry, id: &str, module_type: &str, enabled: Option<bool>) {
    let desired_state = match enabled {
        Some(true) => "running",
        Some(false) => "stopped",
        None => "disabled",
    };
    let descriptor = ModuleDescriptor::new(id, module_type, desired_state, desired_state);
    registry.register_module(ModuleRegistration::new(descriptor));
}

fn register_codec(registry: &Registry, id: &str, enabled: bool) {
    let desired_state = if enabled { "available" } else { "disabled" };
    let descriptor = ModuleDescriptor::new(id, "codec", desired_state, desired_state);
    registry.register_module(ModuleRegistration::new(descriptor));
}

fn start_srt_in(cfg: &Config, context: &AppContext, running: Arc<AtomicBool>) {
    if let Some(srt_cfg) = &cfg.srt_in {
        context
            .control_state
            .srt_in
            .module
            .set_enabled(srt_cfg.enabled);
        if srt_cfg.enabled {
            info!("[airlift] SRT enabled: {}", srt_cfg.listen);

            let ring = context.agent.ring.clone();
            let cfg_clone = srt_cfg.clone();
            let srt_state = context.control_state.srt_in.clone();
            let ring_state = context.control_state.ring.clone();

            std::thread::spawn(move || {
                if let Err(e) =
                    crate::io::srt_in::run_srt_in(ring, cfg_clone, running, srt_state, ring_state)
                {
                    error!("[srt] fatal: {}", e);
                }
            });
        }
    } else {
        context.control_state.srt_in.module.set_enabled(false);
    }
}

fn start_alsa_in(cfg: &Config, context: &AppContext) {
    if let Some(alsa_cfg) = &cfg.alsa_in {
        context.control_state.alsa_in.set_enabled(alsa_cfg.enabled);
        if alsa_cfg.enabled {
            info!("[airlift] ALSA enabled: {}", alsa_cfg.device);

            let ring = context.agent.ring.clone();
            let metrics_alsa = context.metrics.clone();
            let alsa_state = context.control_state.alsa_in.clone();
            let ring_state = context.control_state.ring.clone();

            std::thread::spawn(move || {
                if let Err(e) =
                    crate::io::alsa_in::run_alsa_in(ring, metrics_alsa, alsa_state, ring_state)
                {
                    error!("[alsa] fatal: {}", e);
                }
            });
        }
    } else {
        context.control_state.alsa_in.set_enabled(false);
    }
}

fn sync_output_states(cfg: &Config, context: &AppContext) {
    if let Some(srt_out_cfg) = &cfg.srt_out {
        context
            .control_state
            .srt_out
            .module
            .set_enabled(srt_out_cfg.enabled);
    } else {
        context.control_state.srt_out.module.set_enabled(false);
    }

    if let Some(icecast_cfg) = &cfg.icecast_out {
        context
            .control_state
            .icecast_out
            .set_enabled(icecast_cfg.enabled);
    } else {
        context.control_state.icecast_out.set_enabled(false);
    }
}

fn start_peak_storage(context: &AppContext) {
    let reader = context.agent.ring.subscribe();
    let store = context.peak_store.clone();

    let influx = InfluxOut::new(
        "http://localhost:8086/write".into(),
        "rfm_aircheck".into(),
        100,
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

fn start_recorder(cfg: &Config, context: &AppContext) -> anyhow::Result<()> {
    let Some(c) = &cfg.recorder else {
        context.control_state.recorder.set_enabled(false);
        return Ok(());
    };
    if !c.enabled {
        context.control_state.recorder.set_enabled(false);
        return Ok(());
    }
    context.control_state.recorder.set_enabled(true);

    let reader = context.agent.ring.subscribe();

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

    let recorder_state = context.control_state.recorder.clone();
    let ring_state = context.control_state.ring.clone();
    std::thread::spawn(move || {
        if let Err(e) = run_recorder(
            reader,
            rec_cfg,
            sinks,
            retentions,
            recorder_state,
            ring_state,
        ) {
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
