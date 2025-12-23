use std::path::PathBuf;
use std::sync::{Arc, Mutex, atomic::AtomicBool};
use std::time::Duration;

use log::{error, info};

use crate::agent;
use crate::api::registry::{ModuleDescriptor, ModuleRegistration, Registry};
use crate::codecs::registry::{CodecRegistry, resolve_codec_configs};
use crate::config::{Config, ValidatedGraphConfig};
use crate::control::{ControlState, ModuleState};
use crate::io::broadcast_http::BroadcastHttp;
use crate::io::file_in::FileInConfig;
use crate::io::file_out::{self, FileOutConfig, FsRetention, run_file_out};
use crate::io::influx_out::InfluxOut;
use crate::io::peak_analyzer::{PeakAnalyzer, PeakEvent};
use crate::monitoring;
use crate::ring::{EncodedRingRead, EncodedRingReader, RingRead};
use crate::web::influx_service::InfluxHistoryService;
use crate::web::peaks::{PeakPoint, PeakStorage};

pub struct AppContext {
    pub agent: agent::Agent,
    pub metrics: Arc<monitoring::Metrics>,
    pub control_state: Arc<ControlState>,
    pub peak_store: Arc<PeakStorage>,
    pub history_service: Option<Arc<InfluxHistoryService>>,
    pub wav_dir: PathBuf,
    pub codec_registry: Arc<CodecRegistry>,
}

impl AppContext {
    pub fn new(cfg: &Config, agent: agent::Agent, codec_registry: Arc<CodecRegistry>) -> Self {
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
            .file_out
            .as_ref()
            .map(|file_out| PathBuf::from(&file_out.wav_dir))
            .unwrap_or_else(|| PathBuf::from("/data/aircheck/wav"));

        Self {
            agent,
            metrics,
            control_state,
            peak_store,
            history_service,
            wav_dir,
            codec_registry,
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
        "file_out",
        "file_out",
        cfg.file_out.as_ref().map(|c| c.enabled),
    );

    // Codec-Module als eigenständige Processing-Instanzen.
    for codec in resolve_codec_configs(cfg) {
        register_codec(registry, &codec.id, true);
    }
}

pub fn register_graph_modules(graph: &ValidatedGraphConfig, registry: &Registry) {
    let ring_descriptor = ModuleDescriptor::new(
        graph.ringbuffer_id.clone(),
        "ringbuffer",
        "running",
        "running",
    );
    registry.register_module(ModuleRegistration::new(ring_descriptor));

    for (id, input) in graph.inputs.iter() {
        register_toggle_module(registry, id, &input.input_type, Some(input.enabled));
    }
    for (id, output) in graph.outputs.iter() {
        register_toggle_module(registry, id, &output.output_type, Some(output.enabled));
    }
    for (id, service) in graph.services.iter() {
        register_toggle_module(registry, id, &service.service_type, Some(service.enabled));
    }
    for codec in graph.codecs.iter() {
        register_codec(registry, &codec.id, true);
    }
}

pub fn start_graph_workers(
    cfg: &Config,
    graph: &ValidatedGraphConfig,
    context: &AppContext,
    running: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    start_graph_inputs(graph, context, running.clone())?;
    start_graph_outputs(graph, context, running.clone())?;
    start_graph_services(cfg, graph, context, running)?;
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

#[derive(Default)]
struct PeakEventFanout {
    handlers: Mutex<Vec<Box<dyn Fn(&PeakEvent) + Send + Sync>>>,
}

impl PeakEventFanout {
    fn add_handler(&self, handler: Box<dyn Fn(&PeakEvent) + Send + Sync>) {
        let mut handlers = self.handlers.lock().expect("peak_event handlers lock");
        handlers.push(handler);
    }

    fn emit(&self, evt: &PeakEvent) {
        let handlers = self.handlers.lock().expect("peak_event handlers lock");
        for handler in handlers.iter() {
            handler(evt);
        }
    }
}

struct GraphEncodedSource {
    reader: EncodedRingReader,
}

impl GraphEncodedSource {
    fn new(reader: EncodedRingReader) -> Self {
        Self { reader }
    }

    fn poll_next(&mut self) -> EncodedRingRead {
        self.reader.poll()
    }
}

impl crate::io::srt_out::EncodedFrameSource for GraphEncodedSource {
    fn poll(&mut self) -> anyhow::Result<crate::io::srt_out::EncodedRead> {
        match self.poll_next() {
            EncodedRingRead::Frame { frame, .. } => {
                Ok(crate::io::srt_out::EncodedRead::Frame(frame))
            }
            EncodedRingRead::Gap { missed } => Ok(crate::io::srt_out::EncodedRead::Gap { missed }),
            EncodedRingRead::Empty => Ok(crate::io::srt_out::EncodedRead::Empty),
        }
    }
}

impl crate::io::udp_out::EncodedFrameSource for GraphEncodedSource {
    fn poll(&mut self) -> anyhow::Result<crate::io::udp_out::EncodedRead> {
        match self.poll_next() {
            EncodedRingRead::Frame { frame, .. } => {
                Ok(crate::io::udp_out::EncodedRead::Frame(frame))
            }
            EncodedRingRead::Gap { missed } => Ok(crate::io::udp_out::EncodedRead::Gap { missed }),
            EncodedRingRead::Empty => Ok(crate::io::udp_out::EncodedRead::Empty),
        }
    }
}

impl crate::io::icecast_out::EncodedFrameSource for GraphEncodedSource {
    fn poll(&mut self) -> anyhow::Result<crate::io::icecast_out::EncodedRead> {
        match self.poll_next() {
            EncodedRingRead::Frame { frame, .. } => {
                Ok(crate::io::icecast_out::EncodedRead::Frame(frame))
            }
            EncodedRingRead::Gap { missed } => {
                Ok(crate::io::icecast_out::EncodedRead::Gap { missed })
            }
            EncodedRingRead::Empty => Ok(crate::io::icecast_out::EncodedRead::Empty),
        }
    }
}

impl crate::audio::http::EncodedFrameSource for GraphEncodedSource {
    fn poll(&mut self) -> anyhow::Result<crate::audio::http::EncodedRead> {
        match self.poll_next() {
            EncodedRingRead::Frame { frame, .. } => {
                Ok(crate::audio::http::EncodedRead::Frame(frame))
            }
            EncodedRingRead::Gap { missed } => {
                Ok(crate::audio::http::EncodedRead::Gap { missed })
            }
            EncodedRingRead::Empty => Ok(crate::audio::http::EncodedRead::Empty),
        }
    }
}

fn start_graph_inputs(
    graph: &ValidatedGraphConfig,
    context: &AppContext,
    running: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    let ring = context.agent.ring.clone();
    let ring_state = context.control_state.ring.clone();

    for (_id, input) in graph.inputs.iter() {
        if !input.enabled {
            continue;
        }

        match input.input_type.as_str() {
            "srt" => {
                let srt_cfg = crate::config::SrtInConfig {
                    enabled: input.enabled,
                    listen: input
                        .listen
                        .clone()
                        .unwrap_or_else(|| "0.0.0.0:9000".to_string()),
                    latency_ms: input.latency_ms.unwrap_or(200),
                };
                context.control_state.srt_in.module.set_enabled(true);
                let srt_state = context.control_state.srt_in.clone();
                let ring_clone = ring.clone();
                let running_clone = running.clone();
                let ring_state = ring_state.clone();
                std::thread::spawn(move || {
                    if let Err(e) = crate::io::srt_in::run_srt_in(
                        ring_clone,
                        srt_cfg,
                        running_clone,
                        srt_state,
                        ring_state,
                    ) {
                        error!("[srt] fatal: {}", e);
                    }
                });
            }
            "icecast" | "http_stream" => {
                let url = input
                    .url
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("input requires url"))?;
                context.control_state.icecast_in.set_enabled(true);
                let state = context.control_state.icecast_in.clone();
                let ring_clone = ring.clone();
                let running_clone = running.clone();
                let ring_state = ring_state.clone();
                std::thread::spawn(move || {
                    if let Err(e) = crate::io::icecast_in::run_icecast_in(
                        ring_clone,
                        url,
                        running_clone,
                        state,
                        ring_state,
                    ) {
                        error!("[icecast_in] fatal: {}", e);
                    }
                });
            }
            #[cfg(feature = "alsa")]
            "alsa" => {
                let alsa_cfg = crate::config::AlsaInConfig {
                    enabled: input.enabled,
                    device: input.device.clone().unwrap_or_default(),
                };
                context.control_state.alsa_in.set_enabled(true);
                let metrics_alsa = context.metrics.clone();
                let alsa_state = context.control_state.alsa_in.clone();
                let ring_state = ring_state.clone();
                let ring_clone = ring.clone();
                std::thread::spawn(move || {
                    if let Err(e) = crate::io::alsa_in::run_alsa_in(
                        ring_clone,
                        metrics_alsa,
                        alsa_state,
                        ring_state,
                    ) {
                        error!("[alsa] fatal: {}", e);
                    }
                });
            }
            #[cfg(not(feature = "alsa"))]
            "alsa" => {
                context.control_state.alsa_in.set_enabled(false);
            }
            "file_in" => {
                let path = input
                    .path
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("input requires path"))?;
                context.control_state.file_in.set_enabled(true);
                let ring_clone = ring.clone();
                let state = context.control_state.file_in.clone();
                let running_clone = running.clone();
                let ring_state = ring_state.clone();
                std::thread::spawn(move || {
                    let cfg = FileInConfig {
                        enabled: true,
                        path: PathBuf::from(path),
                    };
                    if let Err(e) = crate::io::file_in::run_file_in(
                        ring_clone,
                        cfg,
                        running_clone,
                        state,
                        ring_state,
                    ) {
                        error!("[file_in] fatal: {}", e);
                    }
                });
            }
            _ => {}
        }
    }

    Ok(())
}

fn start_graph_outputs(
    graph: &ValidatedGraphConfig,
    context: &AppContext,
    running: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    let ring_state = context.control_state.ring.clone();
    let encoded_ring = context.agent.encoded_ring.clone();

    for (_id, output) in graph.outputs.iter() {
        if !output.enabled {
            continue;
        }

        let codec_id = output
            .codec_id
            .clone()
            .ok_or_else(|| anyhow::anyhow!("output requires codec_id"))?;
        let encoded_reader = GraphEncodedSource::new(encoded_ring.subscribe());

        match output.output_type.as_str() {
            "srt_out" => {
                context.control_state.srt_out.module.set_enabled(true);
                let cfg = crate::config::SrtOutConfig {
                    enabled: output.enabled,
                    target: output.target.clone().unwrap_or_default(),
                    latency_ms: output.latency_ms.unwrap_or(200),
                    codec_id: output.codec_id.clone(),
                };
                let state = context.control_state.srt_out.clone();
                let ring_state = ring_state.clone();
                let running = running.clone();
                let registry = context.codec_registry.clone();
                std::thread::spawn(move || {
                    if let Err(e) = crate::io::srt_out::run_srt_out(
                        encoded_reader,
                        cfg,
                        running,
                        state,
                        ring_state,
                        registry,
                    ) {
                        error!("[srt_out] fatal: {}", e);
                    }
                });
            }
            "icecast_out" => {
                context.control_state.icecast_out.set_enabled(true);
                let cfg = crate::io::icecast_out::IcecastConfig {
                    host: output.host.clone().unwrap_or_default(),
                    port: output.port.unwrap_or_default(),
                    mount: output.mount.clone().unwrap_or_default(),
                    user: output.user.clone().unwrap_or_default(),
                    password: output.password.clone().unwrap_or_default(),
                    name: output.name.clone().unwrap_or_default(),
                    description: output.description.clone().unwrap_or_default(),
                    genre: output.genre.clone().unwrap_or_default(),
                    public: output.public.unwrap_or(false),
                    opus_bitrate: output.bitrate.unwrap_or(96_000) as i32,
                    codec_id: output.codec_id.clone(),
                };
                let state = context.control_state.icecast_out.clone();
                let ring_state = ring_state.clone();
                let registry = context.codec_registry.clone();
                std::thread::spawn(move || {
                    if let Err(e) = crate::io::icecast_out::run_icecast_out(
                        encoded_reader,
                        cfg,
                        state,
                        ring_state,
                        registry,
                    ) {
                        error!("[icecast] fatal: {}", e);
                    }
                });
            }
            "udp_out" => {
                let target = output.target.clone().unwrap_or_default();
                let codec_id = output.codec_id.clone();
                let metrics = context.metrics.clone();
                let registry = context.codec_registry.clone();
                std::thread::spawn(move || {
                    if let Err(e) = crate::io::udp_out::run_udp_out(
                        encoded_reader,
                        &target,
                        codec_id.as_deref(),
                        metrics,
                        registry,
                    ) {
                        error!("[udp] fatal: {}", e);
                    }
                });
            }
            "file_out" => {
                start_file_out_worker(
                    context,
                    ring_state.clone(),
                    codec_id.clone(),
                    output.wav_dir.clone().unwrap_or_default(),
                    output.retention_days.unwrap_or(0),
                )?;
            }
            _ => {}
        }
    }

    Ok(())
}

fn start_file_out_worker(
    context: &AppContext,
    ring_state: Arc<ModuleState>,
    codec_id: String,
    wav_dir: String,
    retention_days: u64,
) -> anyhow::Result<()> {
    context.control_state.file_out.set_enabled(true);
    let base_dir = PathBuf::from(wav_dir);
    let codec_info = context.codec_registry.get_info(&codec_id)?;
    if matches!(codec_info.container, crate::codecs::ContainerKind::Rtp) {
        anyhow::bail!(
            "file_out does not support RTP container (codec_id '{}')",
            codec_id
        );
    }
    let retentions: Vec<Box<dyn file_out::RetentionPolicy>> = vec![Box::new(FsRetention::new(
        base_dir.clone(),
        retention_days,
    ))];
    let file_out_cfg = FileOutConfig {
        idle_sleep: Duration::from_millis(5),
        retention_interval: Duration::from_secs(3600),
    };
    let state = context.control_state.file_out.clone();
    let source = context.agent.encoded_ring.subscribe();
    std::thread::spawn(move || {
        if let Err(e) = run_file_out(
            source,
            file_out_cfg,
            base_dir,
            codec_id,
            codec_info,
            retentions,
            state,
            ring_state,
        ) {
            error!("[file_out] fatal: {}", e);
        }
    });
    Ok(())
}

fn start_graph_services(
    cfg: &Config,
    graph: &ValidatedGraphConfig,
    context: &AppContext,
    running: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    let peak_events = Arc::new(PeakEventFanout::default());
    let peak_store = context.peak_store.clone();
    let ring_state = context.control_state.ring.clone();
    peak_events.add_handler(Box::new(move |evt: &PeakEvent| {
        peak_store.add_peak(PeakPoint {
            timestamp: evt.utc_ns / 1_000_000,
            peak_l: evt.peak_l,
            peak_r: evt.peak_r,
            rms: None,
            lufs: None,
            silence: evt.silence,
        });
    }));

    if cfg.audio_http.enabled {
        let audio_http = graph
            .services
            .iter()
            .find(|(_, svc)| svc.service_type == "audio_http");
        if let Some((_id, service)) = audio_http {
            if service.enabled {
                let audio_bind = cfg.audio_http.bind.clone();
                let audio_bind_for_thread = audio_bind.clone();
                let encoded_ring = context.agent.encoded_ring.clone();
                let wav_dir = context.wav_dir.clone();
                let codec_id = service
                    .codec_id
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("service requires codec_id"))?;
                let codec_registry = context.codec_registry.clone();
                std::thread::spawn(move || {
                    if let Err(e) = crate::audio::http::start_audio_http_server(
                        &audio_bind_for_thread,
                        wav_dir,
                        move || {
                            GraphEncodedSource::new(encoded_ring.subscribe())
                        },
                        Some(codec_id),
                        codec_registry,
                    ) {
                        error!("[audio_http] server failed: {}", e);
                    }
                });
                info!("[airlift] audio HTTP enabled (http://{})", audio_bind);
            }
        }
    }

    if cfg.monitoring.enabled {
        let monitoring = graph
            .services
            .iter()
            .find(|(_, svc)| svc.service_type == "monitoring");
        if let Some((_id, service)) = monitoring {
            if service.enabled {
                crate::services::MonitoringService::start(
                    cfg,
                    &context.agent,
                    context.metrics.clone(),
                    running,
                )?;
            }
        }
    }

    for (_id, service) in graph.services.iter() {
        if !service.enabled {
            continue;
        }

        match service.service_type.as_str() {
            "influx_out" => {
                let url = service
                    .url
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("service requires url"))?;
                let db = service
                    .db
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("service requires db"))?;
                let interval_ms = service
                    .interval_ms
                    .ok_or_else(|| anyhow::anyhow!("service requires interval_ms"))?;
                let influx = Arc::new(InfluxOut::new(url, db, interval_ms));
                let handler = Box::new(move |evt: &PeakEvent| {
                    influx.handle(evt);
                });
                peak_events.add_handler(handler);
            }
            "broadcast_http" => {
                let url = service
                    .url
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("service requires url"))?;
                let interval_ms = service
                    .interval_ms
                    .ok_or_else(|| anyhow::anyhow!("service requires interval_ms"))?;
                let broadcast = Arc::new(BroadcastHttp::new(url, interval_ms));
                let handler = Box::new(move |evt: &PeakEvent| {
                    broadcast.handle(evt);
                });
                peak_events.add_handler(handler);
            }
            "file_out" => {
                let codec_id = service
                    .codec_id
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("service requires codec_id"))?;
                start_file_out_worker(
                    context,
                    ring_state.clone(),
                    codec_id,
                    service.wav_dir.clone().unwrap_or_default(),
                    service.retention_days.unwrap_or(0),
                )?;
            }
            "monitor" => {
                let mut reader = context.agent.ring.subscribe();
                let running = running.clone();
                std::thread::spawn(move || {
                    while running.load(std::sync::atomic::Ordering::Relaxed) {
                        match reader.poll() {
                            RingRead::Chunk(_) => {}
                            RingRead::Gap { .. } => {}
                            RingRead::Empty => std::thread::sleep(Duration::from_millis(10)),
                        }
                    }
                });
            }
            _ => {}
        }
    }

    if let Some((_id, service)) = graph
        .services
        .iter()
        .find(|(_, svc)| svc.service_type == "peak_analyzer")
    {
        if service.enabled {
            let interval_ms = service
                .interval_ms
                .ok_or_else(|| anyhow::anyhow!("service requires interval_ms"))?;
            let reader = context.agent.ring.subscribe();
            let peak_events = peak_events.clone();
            let handler = Box::new(move |evt: &PeakEvent| {
                peak_events.emit(evt);
            });
            let mut analyzer = PeakAnalyzer::new(reader, handler, interval_ms);
            std::thread::spawn(move || analyzer.run());
        }
    }

    Ok(())
}
