use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, atomic::AtomicBool};
use std::time::Duration;

use log::{error, info};

use crate::agent;
use crate::api::registry::{ModuleDescriptor, ModuleRegistration, Registry};
use crate::codecs::AudioCodec;
use crate::codecs::registry::{CodecInstance, CodecRegistry, resolve_codec_configs};
use crate::config::{Config, ValidatedGraphConfig};
use crate::control::ControlState;
use crate::io::broadcast_http::BroadcastHttp;
use crate::io::influx_out::InfluxOut;
use crate::io::peak_analyzer::{PeakAnalyzer, PeakEvent};
use crate::monitoring;
use crate::recorder::{
    self, EncodedFrameSource, EncodedRead, FsRetention, RecorderConfig, run_recorder,
};
use crate::ring::{EncodedRingRead, EncodedRingReader, RingRead, RingReader};
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
        "recorder",
        "recorder",
        cfg.recorder.as_ref().map(|c| c.enabled),
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

#[cfg(feature = "alsa")]
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

#[cfg(not(feature = "alsa"))]
fn start_alsa_in(_cfg: &Config, context: &AppContext) {
    context.control_state.alsa_in.set_enabled(false);
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
    // Legacy: only started when the Graph-Pipeline is not active.
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

enum RingEncodedRead {
    Frame {
        frame: crate::codecs::EncodedFrame,
        utc_ns: u64,
    },
    Gap {
        missed: u64,
    },
    Empty,
}

struct EncodedRingSource {
    reader: RingReader,
    codec: Arc<Mutex<Box<dyn AudioCodec>>>,
    codec_instance: Arc<CodecInstance>,
    pending: VecDeque<crate::codecs::EncodedFrame>,
    pending_utc_ns: Option<u64>,
}

impl EncodedRingSource {
    fn new(
        reader: RingReader,
        codec: Arc<Mutex<Box<dyn AudioCodec>>>,
        codec_instance: Arc<CodecInstance>,
    ) -> Self {
        Self {
            reader,
            codec,
            codec_instance,
            pending: VecDeque::new(),
            pending_utc_ns: None,
        }
    }

    fn poll_next(&mut self) -> anyhow::Result<RingEncodedRead> {
        if let Some(frame) = self.pending.pop_front() {
            let utc_ns = self.pending_utc_ns.unwrap_or_default();
            return Ok(RingEncodedRead::Frame { frame, utc_ns });
        }

        match self.reader.poll() {
            RingRead::Chunk(slot) => {
                let frames = {
                    let mut codec = self.codec.lock().expect("output codec lock");
                    codec.encode(&slot.pcm)
                };
                let frames = match frames {
                    Ok(frames) => frames,
                    Err(e) => {
                        self.codec_instance.mark_error(&e.to_string());
                        return Err(e);
                    }
                };

                let bytes: u64 = frames.iter().map(|f| f.payload.len() as u64).sum();
                let frame_count = frames.len() as u64;
                self.codec_instance.mark_encoded(1, frame_count, bytes);

                if frames.is_empty() {
                    return Ok(RingEncodedRead::Empty);
                }
                self.pending_utc_ns = Some(slot.utc_ns);
                self.pending = frames.into_iter().collect();
                let frame = self.pending.pop_front().expect("output pending frame");
                Ok(RingEncodedRead::Frame {
                    frame,
                    utc_ns: slot.utc_ns,
                })
            }
            RingRead::Gap { missed } => Ok(RingEncodedRead::Gap { missed }),
            RingRead::Empty => Ok(RingEncodedRead::Empty),
        }
    }
}

impl crate::io::srt_out::EncodedFrameSource for EncodedRingSource {
    fn poll(&mut self) -> anyhow::Result<crate::io::srt_out::EncodedRead> {
        match self.poll_next()? {
            RingEncodedRead::Frame { frame, .. } => {
                Ok(crate::io::srt_out::EncodedRead::Frame(frame))
            }
            RingEncodedRead::Gap { missed } => Ok(crate::io::srt_out::EncodedRead::Gap { missed }),
            RingEncodedRead::Empty => Ok(crate::io::srt_out::EncodedRead::Empty),
        }
    }
}

impl crate::io::udp_out::EncodedFrameSource for EncodedRingSource {
    fn poll(&mut self) -> anyhow::Result<crate::io::udp_out::EncodedRead> {
        match self.poll_next()? {
            RingEncodedRead::Frame { frame, .. } => {
                Ok(crate::io::udp_out::EncodedRead::Frame(frame))
            }
            RingEncodedRead::Gap { missed } => Ok(crate::io::udp_out::EncodedRead::Gap { missed }),
            RingEncodedRead::Empty => Ok(crate::io::udp_out::EncodedRead::Empty),
        }
    }
}

impl crate::io::icecast_out::EncodedFrameSource for EncodedRingSource {
    fn poll(&mut self) -> anyhow::Result<crate::io::icecast_out::EncodedRead> {
        match self.poll_next()? {
            RingEncodedRead::Frame { frame, .. } => {
                Ok(crate::io::icecast_out::EncodedRead::Frame(frame))
            }
            RingEncodedRead::Gap { missed } => {
                Ok(crate::io::icecast_out::EncodedRead::Gap { missed })
            }
            RingEncodedRead::Empty => Ok(crate::io::icecast_out::EncodedRead::Empty),
        }
    }
}

impl recorder::EncodedFrameSource for EncodedRingSource {
    fn poll(&mut self) -> anyhow::Result<recorder::EncodedRead> {
        match self.poll_next()? {
            RingEncodedRead::Frame { frame, utc_ns } => {
                Ok(recorder::EncodedRead::Frame { frame, utc_ns })
            }
            RingEncodedRead::Gap { missed } => Ok(recorder::EncodedRead::Gap { missed }),
            RingEncodedRead::Empty => Ok(recorder::EncodedRead::Empty),
        }
    }
}

struct PassthroughEncodedRingSource {
    reader: EncodedRingReader,
}

impl PassthroughEncodedRingSource {
    fn new(reader: EncodedRingReader) -> Self {
        Self { reader }
    }

    fn poll_next(&mut self) -> EncodedRingRead {
        self.reader.poll()
    }
}

enum GraphEncodedSource {
    Encoded(EncodedRingSource),
    Passthrough(PassthroughEncodedRingSource),
}

impl GraphEncodedSource {
    fn from_encoded(reader: EncodedRingReader) -> Self {
        Self::Passthrough(PassthroughEncodedRingSource::new(reader))
    }
}

impl crate::io::srt_out::EncodedFrameSource for GraphEncodedSource {
    fn poll(&mut self) -> anyhow::Result<crate::io::srt_out::EncodedRead> {
        match self {
            GraphEncodedSource::Encoded(source) => match source.poll_next()? {
                RingEncodedRead::Frame { frame, .. } => {
                    Ok(crate::io::srt_out::EncodedRead::Frame(frame))
                }
                RingEncodedRead::Gap { missed } => {
                    Ok(crate::io::srt_out::EncodedRead::Gap { missed })
                }
                RingEncodedRead::Empty => Ok(crate::io::srt_out::EncodedRead::Empty),
            },
            GraphEncodedSource::Passthrough(source) => match source.poll_next() {
                EncodedRingRead::Frame { frame, .. } => {
                    Ok(crate::io::srt_out::EncodedRead::Frame(frame))
                }
                EncodedRingRead::Gap { missed } => {
                    Ok(crate::io::srt_out::EncodedRead::Gap { missed })
                }
                EncodedRingRead::Empty => Ok(crate::io::srt_out::EncodedRead::Empty),
            },
        }
    }
}

impl crate::io::udp_out::EncodedFrameSource for GraphEncodedSource {
    fn poll(&mut self) -> anyhow::Result<crate::io::udp_out::EncodedRead> {
        match self {
            GraphEncodedSource::Encoded(source) => match source.poll_next()? {
                RingEncodedRead::Frame { frame, .. } => {
                    Ok(crate::io::udp_out::EncodedRead::Frame(frame))
                }
                RingEncodedRead::Gap { missed } => {
                    Ok(crate::io::udp_out::EncodedRead::Gap { missed })
                }
                RingEncodedRead::Empty => Ok(crate::io::udp_out::EncodedRead::Empty),
            },
            GraphEncodedSource::Passthrough(source) => match source.poll_next() {
                EncodedRingRead::Frame { frame, .. } => {
                    Ok(crate::io::udp_out::EncodedRead::Frame(frame))
                }
                EncodedRingRead::Gap { missed } => {
                    Ok(crate::io::udp_out::EncodedRead::Gap { missed })
                }
                EncodedRingRead::Empty => Ok(crate::io::udp_out::EncodedRead::Empty),
            },
        }
    }
}

impl crate::io::icecast_out::EncodedFrameSource for GraphEncodedSource {
    fn poll(&mut self) -> anyhow::Result<crate::io::icecast_out::EncodedRead> {
        match self {
            GraphEncodedSource::Encoded(source) => match source.poll_next()? {
                RingEncodedRead::Frame { frame, .. } => {
                    Ok(crate::io::icecast_out::EncodedRead::Frame(frame))
                }
                RingEncodedRead::Gap { missed } => {
                    Ok(crate::io::icecast_out::EncodedRead::Gap { missed })
                }
                RingEncodedRead::Empty => Ok(crate::io::icecast_out::EncodedRead::Empty),
            },
            GraphEncodedSource::Passthrough(source) => match source.poll_next() {
                EncodedRingRead::Frame { frame, .. } => {
                    Ok(crate::io::icecast_out::EncodedRead::Frame(frame))
                }
                EncodedRingRead::Gap { missed } => {
                    Ok(crate::io::icecast_out::EncodedRead::Gap { missed })
                }
                EncodedRingRead::Empty => Ok(crate::io::icecast_out::EncodedRead::Empty),
            },
        }
    }
}

impl recorder::EncodedFrameSource for GraphEncodedSource {
    fn poll(&mut self) -> anyhow::Result<recorder::EncodedRead> {
        match self {
            GraphEncodedSource::Encoded(source) => match source.poll_next()? {
                RingEncodedRead::Frame { frame, utc_ns } => {
                    Ok(recorder::EncodedRead::Frame { frame, utc_ns })
                }
                RingEncodedRead::Gap { missed } => Ok(recorder::EncodedRead::Gap { missed }),
                RingEncodedRead::Empty => Ok(recorder::EncodedRead::Empty),
            },
            GraphEncodedSource::Passthrough(source) => match source.poll_next() {
                EncodedRingRead::Frame { frame, utc_ns } => {
                    Ok(recorder::EncodedRead::Frame { frame, utc_ns })
                }
                EncodedRingRead::Gap { missed } => Ok(recorder::EncodedRead::Gap { missed }),
                EncodedRingRead::Empty => Ok(recorder::EncodedRead::Empty),
            },
        }
    }
}

impl crate::audio::http::EncodedFrameSource for GraphEncodedSource {
    fn poll(&mut self) -> anyhow::Result<crate::audio::http::EncodedRead> {
        match self {
            GraphEncodedSource::Encoded(source) => match source.poll_next()? {
                RingEncodedRead::Frame { frame, .. } => {
                    Ok(crate::audio::http::EncodedRead::Frame(frame))
                }
                RingEncodedRead::Gap { missed } => {
                    Ok(crate::audio::http::EncodedRead::Gap { missed })
                }
                RingEncodedRead::Empty => Ok(crate::audio::http::EncodedRead::Empty),
            },
            GraphEncodedSource::Passthrough(source) => match source.poll_next() {
                EncodedRingRead::Frame { frame, .. } => {
                    Ok(crate::audio::http::EncodedRead::Frame(frame))
                }
                EncodedRingRead::Gap { missed } => {
                    Ok(crate::audio::http::EncodedRead::Gap { missed })
                }
                EncodedRingRead::Empty => Ok(crate::audio::http::EncodedRead::Empty),
            },
        }
    }
}

struct RecorderEncoderSource {
    reader: RingReader,
    codec: Arc<Mutex<Box<dyn AudioCodec>>>,
    codec_instance: Arc<crate::codecs::registry::CodecInstance>,
    pending: VecDeque<crate::codecs::EncodedFrame>,
    pending_utc_ns: Option<u64>,
}

impl RecorderEncoderSource {
    fn new(
        reader: RingReader,
        codec: Arc<Mutex<Box<dyn AudioCodec>>>,
        codec_instance: Arc<crate::codecs::registry::CodecInstance>,
    ) -> Self {
        Self {
            reader,
            codec,
            codec_instance,
            pending: VecDeque::new(),
            pending_utc_ns: None,
        }
    }
}

impl EncodedFrameSource for RecorderEncoderSource {
    fn poll(&mut self) -> anyhow::Result<EncodedRead> {
        if let Some(frame) = self.pending.pop_front() {
            let utc_ns = self.pending_utc_ns.unwrap_or_default();
            return Ok(EncodedRead::Frame { frame, utc_ns });
        }

        match self.reader.poll() {
            RingRead::Chunk(slot) => {
                let frames = {
                    let mut codec = self.codec.lock().expect("recorder codec lock");
                    codec.encode(&slot.pcm)
                };

                let frames = match frames {
                    Ok(frames) => frames,
                    Err(e) => {
                        self.codec_instance.mark_error(&e.to_string());
                        return Err(e);
                    }
                };

                let bytes: u64 = frames.iter().map(|f| f.payload.len() as u64).sum();
                let frame_count = frames.len() as u64;
                self.codec_instance.mark_encoded(1, frame_count, bytes);

                if frames.is_empty() {
                    return Ok(EncodedRead::Empty);
                }

                self.pending_utc_ns = Some(slot.utc_ns);
                self.pending = frames.into_iter().collect();
                let frame = self.pending.pop_front().expect("recorder pending frame");
                Ok(EncodedRead::Frame {
                    frame,
                    utc_ns: slot.utc_ns,
                })
            }
            RingRead::Gap { missed } => Ok(EncodedRead::Gap { missed }),
            RingRead::Empty => Ok(EncodedRead::Empty),
        }
    }
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

    let codec_id = c
        .codec_id
        .clone()
        .ok_or_else(|| anyhow::anyhow!("recorder requires codec_id"))?;
    let codec_info = context.codec_registry.get_info(&codec_id)?;
    if matches!(codec_info.container, crate::codecs::ContainerKind::Rtp) {
        anyhow::bail!(
            "recorder does not support RTP container (codec_id '{}')",
            codec_id
        );
    }

    let (codec_handle, codec_instance) = context.codec_registry.build_codec_handle(&codec_id)?;
    codec_instance.mark_ready();
    let reader = context.agent.ring.subscribe();
    let encoded_reader = RecorderEncoderSource::new(reader, codec_handle, codec_instance);

    let retentions: Vec<Box<dyn recorder::RetentionPolicy>> = vec![Box::new(FsRetention::new(
        PathBuf::from(&c.wav_dir),
        c.retention_days,
    ))];

    let rec_cfg = RecorderConfig {
        idle_sleep: Duration::from_millis(5),
        retention_interval: Duration::from_secs(3600),
    };

    let recorder_state = context.control_state.recorder.clone();
    let ring_state = context.control_state.ring.clone();
    let base_dir = PathBuf::from(&c.wav_dir);
    std::thread::spawn(move || {
        if let Err(e) = run_recorder(
            encoded_reader,
            rec_cfg,
            base_dir,
            codec_id,
            codec_info,
            retentions,
            recorder_state,
            ring_state,
        ) {
            error!("[recorder] fatal: {}", e);
        }
    });

    info!(
        "[airlift] recorder enabled (codec_id={}, retention {}d)",
        c.codec_id.as_deref().unwrap_or("missing"),
        c.retention_days
    );

    Ok(())
}

fn start_graph_inputs(
    graph: &ValidatedGraphConfig,
    context: &AppContext,
    running: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    let ring = context.agent.ring.clone();
    let encoded_ring = context.agent.encoded_ring.clone();
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
    let ring = context.agent.ring.clone();
    let encoded_ring = context.agent.encoded_ring.clone();
    let uses_encoded_ring = graph_has_encoded_input(graph);

    for (_id, output) in graph.outputs.iter() {
        if !output.enabled {
            continue;
        }

        let codec_id = output
            .codec_id
            .clone()
            .ok_or_else(|| anyhow::anyhow!("output requires codec_id"))?;
        let encoded_reader = if uses_encoded_ring {
            GraphEncodedSource::from_encoded(encoded_ring.subscribe())
        } else {
            let (codec_handle, codec_instance) =
                context.codec_registry.build_codec_handle(&codec_id)?;
            codec_instance.mark_ready();
            let reader = ring.subscribe();
            GraphEncodedSource::Encoded(EncodedRingSource::new(
                reader,
                codec_handle,
                codec_instance,
            ))
        };

        match output.output_type.as_str() {
            "srt_out" => {
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
            "recorder" => {
                let base_dir = PathBuf::from(output.wav_dir.clone().unwrap_or_default());
                let codec_info = context.codec_registry.get_info(&codec_id)?;
                if matches!(codec_info.container, crate::codecs::ContainerKind::Rtp) {
                    anyhow::bail!(
                        "recorder does not support RTP container (codec_id '{}')",
                        codec_id
                    );
                }
                let retentions: Vec<Box<dyn recorder::RetentionPolicy>> = vec![Box::new(
                    FsRetention::new(base_dir.clone(), output.retention_days.unwrap_or(0)),
                )];
                let rec_cfg = RecorderConfig {
                    idle_sleep: Duration::from_millis(5),
                    retention_interval: Duration::from_secs(3600),
                };
                let state = context.control_state.recorder.clone();
                let ring_state = ring_state.clone();
                let codec_id = codec_id.clone();
                std::thread::spawn(move || {
                    if let Err(e) = run_recorder(
                        encoded_reader,
                        rec_cfg,
                        base_dir,
                        codec_id,
                        codec_info,
                        retentions,
                        state,
                        ring_state,
                    ) {
                        error!("[recorder] fatal: {}", e);
                    }
                });
            }
            _ => {}
        }
    }

    Ok(())
}

fn start_graph_services(
    cfg: &Config,
    graph: &ValidatedGraphConfig,
    context: &AppContext,
    running: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    let peak_events = Arc::new(PeakEventFanout::default());

    let audio_http = graph
        .services
        .iter()
        .find(|(_, svc)| svc.service_type == "audio_http");
    if let Some((_id, service)) = audio_http {
        if service.enabled {
            let audio_bind = "0.0.0.0:3011";
            if graph_has_encoded_input(graph) {
                let ring = context.agent.encoded_ring.clone();
                let wav_dir = context.wav_dir.clone();
                let codec_id = service.codec_id.clone();
                let codec_registry = context.codec_registry.clone();
                std::thread::spawn(move || {
                    if let Err(e) = crate::audio::http::start_audio_http_server(
                        audio_bind,
                        wav_dir,
                        move || GraphEncodedSource::from_encoded(ring.subscribe()),
                        codec_id,
                        codec_registry,
                    ) {
                        error!("[audio_http] server failed: {}", e);
                    }
                });
                info!("[airlift] audio HTTP enabled (http://{})", audio_bind);
            } else {
                let audio_http_service = crate::services::AudioHttpService::new(audio_bind);
                audio_http_service.start(
                    context.wav_dir.clone(),
                    context.agent.ring.clone(),
                    service.codec_id.clone(),
                    context.codec_registry.clone(),
                );
            }
        }
    }

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

fn graph_has_encoded_input(graph: &ValidatedGraphConfig) -> bool {
    graph
        .inputs
        .values()
        .any(|input| matches!(input.input_type.as_str(), "icecast" | "http_stream"))
}
